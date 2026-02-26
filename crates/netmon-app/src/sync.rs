use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::auth::AuthManager;
use crate::db::Database;

const API_BASE: &str = "https://api.netmon.app";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub enabled: bool,
    pub last_push: Option<i64>,
    pub last_error: Option<String>,
}

pub struct SyncEngine {
    db: Arc<Mutex<Database>>,
    auth: Arc<AuthManager>,
    running: AtomicBool,
    last_error: Mutex<Option<String>>,
}

#[derive(serde::Serialize)]
struct PushPayload {
    device_id: String,
    device_name: String,
    platform: String,
    targets: Vec<PushTarget>,
    summaries: Vec<PushSummary>,
}

#[derive(serde::Serialize)]
struct PushTarget {
    address: String,
    label: String,
}

#[derive(serde::Serialize)]
struct PushSummary {
    timestamp: i64,
    target: String,
    hop: i32,
    ip: String,
    avg_latency: Option<f64>,
    loss_pct: f64,
    sample_count: i64,
}

impl SyncEngine {
    pub fn new(db: Arc<Mutex<Database>>, auth: Arc<AuthManager>) -> Arc<Self> {
        Arc::new(Self {
            db,
            auth,
            running: AtomicBool::new(false),
            last_error: Mutex::new(None),
        })
    }

    pub fn start(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        let engine = Arc::clone(self);
        std::thread::spawn(move || {
            engine.sync_loop();
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn status(&self) -> SyncStatus {
        let last_push = {
            let db = self.db.lock().unwrap();
            db.get_sync_watermark()
        };

        SyncStatus {
            enabled: self.running.load(Ordering::SeqCst),
            last_push: if last_push > 0 { Some(last_push) } else { None },
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }

    fn sync_loop(self: Arc<Self>) {
        let mut backoff_secs = 5u64;
        let backoff_steps = [5, 15, 60, 300];
        let mut backoff_idx = 0;

        loop {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }

            // Check if authenticated
            let token = match self.auth.get_access_token() {
                Some(t) => t,
                None => {
                    // Not authenticated, wait and retry
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
            };

            match self.push_summaries(&token) {
                Ok(count) => {
                    if count > 0 {
                        *self.last_error.lock().unwrap() = None;
                    }
                    backoff_idx = 0;
                    backoff_secs = backoff_steps[0];
                    // Wait before next push (respect write_rate, default ~5 min)
                    std::thread::sleep(Duration::from_secs(300));
                }
                Err(e) => {
                    eprintln!("Sync push failed: {}", e);
                    *self.last_error.lock().unwrap() = Some(e);
                    std::thread::sleep(Duration::from_secs(backoff_secs));
                    if backoff_idx < backoff_steps.len() - 1 {
                        backoff_idx += 1;
                    }
                    backoff_secs = backoff_steps[backoff_idx];
                }
            }
        }
    }

    fn push_summaries(&self, token: &str) -> Result<usize, String> {
        let db = self.db.lock().unwrap();
        let watermark = db.get_sync_watermark();

        // Get 1-min summaries since last push
        let summaries = db
            .get_summaries_since(watermark)
            .map_err(|e| e.to_string())?;

        if summaries.is_empty() {
            return Ok(0);
        }

        let targets = db.get_active_targets().map_err(|e| e.to_string())?;
        drop(db);

        let device = self.auth.device_info();
        let push_targets: Vec<PushTarget> = targets
            .iter()
            .map(|t| PushTarget {
                address: t.address.clone(),
                label: t.label.clone(),
            })
            .collect();

        let max_ts = summaries.iter().map(|s| s.0).max().unwrap_or(watermark);
        let count = summaries.len();

        let push_summaries: Vec<PushSummary> = summaries
            .into_iter()
            .map(|(ts, target, hop, ip, avg_lat, loss, samples)| PushSummary {
                timestamp: ts,
                target,
                hop,
                ip,
                avg_latency: avg_lat,
                loss_pct: loss,
                sample_count: samples,
            })
            .collect();

        let payload = PushPayload {
            device_id: device.device_id.clone(),
            device_name: device.device_name.clone(),
            platform: device.platform.clone(),
            targets: push_targets,
            summaries: push_summaries,
        };

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}/data/push", API_BASE))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .map_err(|e| format!("Push request failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("Push failed: {}", body));
        }

        // Update watermark
        let db = self.db.lock().unwrap();
        db.set_sync_watermark(max_ts);

        Ok(count)
    }
}
