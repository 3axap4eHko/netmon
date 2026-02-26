use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::db::Database;
use crate::types::LoadTestResult;

const CLOUDFLARE_DOWN: &str = "https://speed.cloudflare.com/__down";
const CLOUDFLARE_UP: &str = "https://speed.cloudflare.com/__up";
const LOAD_THREADS: usize = 8;
const TEST_PHASE_SECS: u64 = 10;
const PROBE_INTERVAL_MS: u64 = 200;
const PROBE_TIMEOUT_SECS: u64 = 5;
const DOWNLOAD_BYTES: u64 = 99_999_999; // ~100MB — Cloudflare rejects >= 100000000

pub struct LoadTestEngine {
    db: Arc<Mutex<Database>>,
    running: AtomicBool,
}

impl LoadTestEngine {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self {
            db,
            running: AtomicBool::new(false),
        }
    }

    /// Spawn the test on a dedicated OS thread (safe for reqwest::blocking)
    /// and return the result via channel.
    pub fn run_test(self: &Arc<Self>) -> Result<LoadTestResult, String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("A load test is already running.".to_string());
        }

        let engine = Arc::clone(self);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        std::thread::spawn(move || {
            let result = engine.run_test_inner();

            engine.running.store(false, Ordering::SeqCst);

            let result = match result {
                Ok(r) => {
                    if let Ok(db) = engine.db.lock() {
                        db.record_load_test(&r).ok();
                    }
                    Ok(r)
                }
                Err(e) => Err(e),
            };
            let _ = tx.send(result);
        });

        rx.recv()
            .map_err(|_| "Load test thread failed".to_string())?
    }

    fn run_test_inner(&self) -> Result<LoadTestResult, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        // Phase 1: Idle latency (~2s)
        let (idle_latency, idle_jitter) = measure_idle_latency(&client)?;

        // Phase 2: Download load test
        let (download_mbps, download_loaded_latency) = measure_download()?;

        // Phase 3: Upload load test
        let (upload_mbps, upload_loaded_latency) = measure_upload()?;

        // Grade
        let worst_loaded = download_loaded_latency.max(upload_loaded_latency);
        let bloat = worst_loaded - idle_latency;
        let grade = compute_grade(bloat);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        Ok(LoadTestResult {
            timestamp,
            idle_latency_ms: round2(idle_latency),
            idle_jitter_ms: round2(idle_jitter),
            download_mbps: round2(download_mbps),
            download_loaded_latency_ms: round2(download_loaded_latency),
            upload_mbps: round2(upload_mbps),
            upload_loaded_latency_ms: round2(upload_loaded_latency),
            grade,
        })
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn compute_grade(bloat_ms: f64) -> String {
    if bloat_ms < 5.0 {
        "A+".to_string()
    } else if bloat_ms < 30.0 {
        "A".to_string()
    } else if bloat_ms < 60.0 {
        "B".to_string()
    } else if bloat_ms < 200.0 {
        "C".to_string()
    } else if bloat_ms < 400.0 {
        "D".to_string()
    } else {
        "F".to_string()
    }
}

fn probe_latency(client: &reqwest::blocking::Client) -> Option<f64> {
    let start = Instant::now();
    let resp = client
        .get(format!("{}?bytes=1", CLOUDFLARE_DOWN))
        .send()
        .ok()?;
    let _ = resp.bytes().ok()?;
    Some(start.elapsed().as_secs_f64() * 1000.0)
}

fn measure_idle_latency(
    client: &reqwest::blocking::Client,
) -> Result<(f64, f64), String> {
    let mut rtts = Vec::new();
    for _ in 0..10 {
        if let Some(rtt) = probe_latency(client) {
            rtts.push(rtt);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if rtts.is_empty() {
        return Err("Failed to measure idle latency — no successful probes.".to_string());
    }
    rtts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = rtts[rtts.len() / 2];

    let jitter = if rtts.len() > 1 {
        let deltas: Vec<f64> = rtts.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        deltas.iter().sum::<f64>() / deltas.len() as f64
    } else {
        0.0
    };

    Ok((median, jitter))
}

/// Writer that counts bytes and can be aborted via AtomicBool
struct ByteCounter {
    stop: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
}

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.stop.load(Ordering::Relaxed) {
            // Must NOT use Interrupted — std::io::copy retries that in a hot loop
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "stopped",
            ));
        }
        self.bytes.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn measure_download() -> Result<(f64, f64), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();
    for _ in 0..LOAD_THREADS {
        let stop = Arc::clone(&stop);
        let bytes = Arc::clone(&total_bytes);
        let handle = std::thread::spawn(move || {
            let worker_client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(TEST_PHASE_SECS + 10))
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .build()
                .ok();
            let Some(c) = worker_client else { return };
            while !stop.load(Ordering::Relaxed) {
                let url = format!("{}?bytes={}", CLOUDFLARE_DOWN, DOWNLOAD_BYTES);
                let resp = c.get(&url).send();
                match resp {
                    Ok(mut r) => {
                        let mut counter = ByteCounter {
                            stop: Arc::clone(&stop),
                            bytes: Arc::clone(&bytes),
                        };
                        // copy_to uses reqwest's internal streaming — reliable
                        let _ = r.copy_to(&mut counter);
                    }
                    Err(_) => {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });
        handles.push(handle);
    }

    // Probe latency during download
    let probe_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Probe client error: {}", e))?;

    let start = Instant::now();
    let mut probe_latencies = Vec::new();
    while start.elapsed().as_secs() < TEST_PHASE_SECS {
        if let Some(rtt) = probe_latency(&probe_client) {
            probe_latencies.push(rtt);
        }
        std::thread::sleep(Duration::from_millis(PROBE_INTERVAL_MS));
    }

    stop.store(true, Ordering::SeqCst);
    for h in handles {
        h.join().ok();
    }

    let elapsed = start.elapsed().as_secs_f64();
    let bytes = total_bytes.load(Ordering::Relaxed);
    let mbps = (bytes as f64 * 8.0) / (elapsed * 1_000_000.0);

    let loaded_latency = trimmed_mean(&probe_latencies);

    Ok((mbps, loaded_latency))
}

fn measure_upload() -> Result<(f64, f64), String> {
    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();
    for _ in 0..LOAD_THREADS {
        let stop = Arc::clone(&stop);
        let bytes = Arc::clone(&total_bytes);
        let handle = std::thread::spawn(move || {
            let worker_client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(TEST_PHASE_SECS + 10))
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .build()
                .ok();
            let Some(c) = worker_client else { return };
            while !stop.load(Ordering::Relaxed) {
                let reader = LoadReader {
                    stop: Arc::clone(&stop),
                    bytes_sent: Arc::clone(&bytes),
                    chunk: [0u8; 262144],
                };
                let body = reqwest::blocking::Body::new(reader);
                let resp = c
                    .post(CLOUDFLARE_UP)
                    .header("Content-Type", "application/octet-stream")
                    .body(body)
                    .send();
                match resp {
                    Ok(_) => {}
                    Err(_) => {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });
        handles.push(handle);
    }

    // Probe latency during upload
    let probe_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Probe client error: {}", e))?;

    let start = Instant::now();
    let mut probe_latencies = Vec::new();
    while start.elapsed().as_secs() < TEST_PHASE_SECS {
        if let Some(rtt) = probe_latency(&probe_client) {
            probe_latencies.push(rtt);
        }
        std::thread::sleep(Duration::from_millis(PROBE_INTERVAL_MS));
    }

    stop.store(true, Ordering::SeqCst);
    for h in handles {
        h.join().ok();
    }

    let elapsed = start.elapsed().as_secs_f64();
    let bytes = total_bytes.load(Ordering::Relaxed);
    let mbps = (bytes as f64 * 8.0) / (elapsed * 1_000_000.0);

    let loaded_latency = trimmed_mean(&probe_latencies);

    Ok((mbps, loaded_latency))
}

/// Trimmed mean: drop the top and bottom 10% of samples
fn trimmed_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let trim = sorted.len() / 10;
    let trimmed = &sorted[trim..sorted.len().saturating_sub(trim)];
    if trimmed.is_empty() {
        sorted.iter().sum::<f64>() / sorted.len() as f64
    } else {
        trimmed.iter().sum::<f64>() / trimmed.len() as f64
    }
}

/// A Read impl that yields 256KB chunks of zeros until stopped
struct LoadReader {
    stop: Arc<AtomicBool>,
    bytes_sent: Arc<AtomicU64>,
    chunk: [u8; 262144],
}

impl Read for LoadReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.stop.load(Ordering::Relaxed) {
            return Ok(0);
        }
        let n = buf.len().min(self.chunk.len());
        buf[..n].copy_from_slice(&self.chunk[..n]);
        self.bytes_sent.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}
