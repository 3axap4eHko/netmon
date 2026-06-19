use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::db::Database;
use crate::http_probe::HttpProber;
use crate::ping::{resolve_ipv4, IcmpPinger};
use crate::types::{HopInfo, PingRecord, ProbeMode, ICMP_PAYLOAD_STANDARD};

const PING_INTERVAL_MS: u64 = 2000;
const MAX_TTL: u8 = 30;
const MAX_CONCURRENT_PINGS: usize = 32;
const DNS_RETRY_MS: i64 = 5 * 60 * 1000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct TargetState {
    hops: Vec<HopInfo>,
    discovering: bool,
    probe_mode: ProbeMode,
}

#[derive(Clone)]
struct HostnameEntry {
    hostname: Option<String>,
    last_attempt_ms: i64,
}

pub struct MtrEngine {
    targets: Mutex<HashMap<String, TargetState>>,
    hostname_cache: Mutex<HashMap<String, HostnameEntry>>,
    paused: AtomicBool,
    running: AtomicBool,
    db: Arc<Mutex<Database>>,
    pinger: Arc<IcmpPinger>,
    http_prober: Arc<HttpProber>,
}

impl MtrEngine {
    pub fn new(db: Arc<Mutex<Database>>, pinger: IcmpPinger, http_prober: HttpProber) -> Arc<Self> {
        Arc::new(Self {
            targets: Mutex::new(HashMap::new()),
            hostname_cache: Mutex::new(HashMap::new()),
            paused: AtomicBool::new(false),
            running: AtomicBool::new(false),
            db,
            pinger: Arc::new(pinger),
            http_prober: Arc::new(http_prober),
        })
    }

    pub fn start(self: &Arc<Self>, targets_with_mode: Vec<(String, ProbeMode)>) {
        {
            let mut targets = self.targets.lock().unwrap();
            for (addr, mode) in &targets_with_mode {
                if !targets.contains_key(addr) {
                    targets.insert(
                        addr.clone(),
                        TargetState {
                            hops: Vec::new(),
                            discovering: false,
                            probe_mode: *mode,
                        },
                    );
                }
            }
        }

        // Discover hops for ICMP targets, set up synthetic hop for HTTP targets
        for (addr, mode) in &targets_with_mode {
            match mode {
                ProbeMode::Http => {
                    self.setup_http_target(addr.clone());
                }
                _ => {
                    self.spawn_discover(addr.clone());
                }
            }
        }

        // Start the ping loop on a background thread
        if !self.running.swap(true, Ordering::SeqCst) {
            let engine = Arc::clone(self);
            std::thread::spawn(move || {
                engine.ping_loop();
            });
        }
    }

    pub fn add_target(self: &Arc<Self>, address: String, probe_mode: ProbeMode) {
        {
            let mut targets = self.targets.lock().unwrap();
            if targets.contains_key(&address) {
                return;
            }
            targets.insert(
                address.clone(),
                TargetState {
                    hops: Vec::new(),
                    discovering: false,
                    probe_mode,
                },
            );
        }
        match probe_mode {
            ProbeMode::Http => self.setup_http_target(address),
            _ => self.spawn_discover(address),
        }
    }

    pub fn remove_target(&self, address: &str) {
        let mut targets = self.targets.lock().unwrap();
        targets.remove(address);
    }

    /// Switch an existing target's probe mode in real time.
    pub fn set_probe_mode(self: &Arc<Self>, address: &str, new_mode: ProbeMode) {
        let mut needs_http_setup = false;
        let mut needs_discovery = false;

        {
            let mut targets = self.targets.lock().unwrap();
            if let Some(target) = targets.get_mut(address) {
                if target.probe_mode == new_mode {
                    return;
                }

                let was_http = target.probe_mode == ProbeMode::Http;
                let is_http = new_mode == ProbeMode::Http;

                target.probe_mode = new_mode;

                if was_http && !is_http {
                    target.hops.clear();
                    target.discovering = false;
                    needs_discovery = true;
                } else if !was_http && is_http {
                    target.hops.clear();
                    target.discovering = false;
                    needs_http_setup = true;
                }
            }
        }

        if needs_http_setup {
            self.setup_http_target(address.to_string());
        }

        if needs_discovery {
            self.spawn_discover(address.to_string());
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(self: &Arc<Self>) {
        self.paused.store(false, Ordering::SeqCst);
        let entries: Vec<(String, ProbeMode)> = {
            let targets = self.targets.lock().unwrap();
            targets.iter().map(|(k, v)| (k.clone(), v.probe_mode)).collect()
        };
        for (addr, mode) in entries {
            match mode {
                ProbeMode::Http => self.setup_http_target(addr),
                _ => self.spawn_discover(addr),
            }
        }
    }

    pub fn get_hostname(self: &Arc<Self>, ip: &str) -> Option<String> {
        let mut should_refresh = false;
        let hostname = {
            let mut cache = self.hostname_cache.lock().unwrap();
            if let Some(entry) = cache.get_mut(ip) {
                if entry.hostname.is_none() && now_ms() - entry.last_attempt_ms >= DNS_RETRY_MS {
                    entry.last_attempt_ms = now_ms();
                    should_refresh = true;
                }
                entry.hostname.clone()
            } else {
                cache.insert(
                    ip.to_string(),
                    HostnameEntry {
                        hostname: None,
                        last_attempt_ms: now_ms(),
                    },
                );
                should_refresh = true;
                None
            }
        };

        if should_refresh {
            let engine = Arc::clone(self);
            let ip_owned = ip.to_string();
            std::thread::spawn(move || {
                engine.resolve_hostname(&ip_owned);
            });
        }

        hostname
    }

    pub fn get_target_hops(&self, address: &str) -> Vec<HopInfo> {
        let targets = self.targets.lock().unwrap();
        targets
            .get(address)
            .map(|target| target.hops.clone())
            .unwrap_or_default()
    }

    fn setup_http_target(&self, address: String) {
        let mut targets = self.targets.lock().unwrap();
        if let Some(t) = targets.get_mut(&address) {
            // HTTP targets get a single synthetic hop — no traceroute discovery
            t.hops = vec![HopInfo {
                hop: 1,
                ip: address.clone(),
            }];
            t.discovering = false;
        }
    }

    fn spawn_discover(self: &Arc<Self>, address: String) {
        let engine = Arc::clone(self);
        let pinger = Arc::clone(&self.pinger);
        std::thread::spawn(move || {
            // Check if already discovering and get payload size
            let payload_size = {
                let mut targets = engine.targets.lock().unwrap();
                if let Some(t) = targets.get_mut(&address) {
                    if t.discovering {
                        return;
                    }
                    t.discovering = true;
                    t.probe_mode.payload_size()
                } else {
                    return;
                }
            };

            // Discovery always uses standard payload to avoid fragmentation issues during traceroute
            let discovery_payload = ICMP_PAYLOAD_STANDARD;
            let _ = payload_size; // actual payload_size used during ping_all_hops

            let target_ip = match resolve_ipv4(&address) {
                Some(ip) => ip,
                None => {
                    let mut targets = engine.targets.lock().unwrap();
                    if let Some(t) = targets.get_mut(&address) {
                        t.discovering = false;
                    }
                    return;
                }
            };

            let mut hops: Vec<HopInfo> = Vec::new();

            for ttl in 1..=MAX_TTL {
                let result = pinger.trace_hop(&target_ip, ttl, discovery_payload);
                if let Some(ip) = result {
                    hops.push(HopInfo {
                        hop: ttl as i32,
                        ip: ip.clone(),
                    });
                    if ip == target_ip {
                        break;
                    }
                } else {
                    hops.push(HopInfo {
                        hop: ttl as i32,
                        ip: "*".to_string(),
                    });
                }
            }

            // Trim trailing * hops
            while hops.last().map(|h| h.ip.as_str()) == Some("*") {
                hops.pop();
            }

            // If traceroute yields no usable hops, keep monitoring alive by
            // falling back to a direct target hop instead of leaving the target
            // permanently stuck in an empty "discovering" state.
            if hops.is_empty() {
                hops.push(HopInfo {
                    hop: 1,
                    ip: target_ip.clone(),
                });
            } else if !hops.iter().any(|hop| hop.ip == target_ip) {
                let next_hop = hops.last().map(|hop| hop.hop + 1).unwrap_or(1);
                hops.push(HopInfo {
                    hop: next_hop,
                    ip: target_ip.clone(),
                });
            }

            // Update target with discovered hops
            {
                let mut targets = engine.targets.lock().unwrap();
                if let Some(t) = targets.get_mut(&address) {
                    t.hops = hops.clone();
                    t.discovering = false;
                }
            }

            // Resolve hostnames in background
            for hop in &hops {
                if hop.ip != "*" {
                    engine.resolve_hostname(&hop.ip);
                }
            }
        });
    }

    fn resolve_hostname(&self, ip: &str) {
        let mut resolved_hostname: Option<String> = None;
        if let Ok(addr) = ip.parse() {
            if let Ok(hostname) = dns_lookup::lookup_addr(&addr) {
                resolved_hostname = Some(hostname);
            }
        }
        let mut cache = self.hostname_cache.lock().unwrap();
        cache.insert(
            ip.to_string(),
            HostnameEntry {
                hostname: resolved_hostname,
                last_attempt_ms: now_ms(),
            },
        );
    }

    fn ping_loop(self: Arc<Self>) {
        let interval = Duration::from_millis(PING_INTERVAL_MS);
        loop {
            let cycle_start = std::time::Instant::now();

            if !self.running.load(Ordering::SeqCst) {
                break;
            }

            if !self.paused.load(Ordering::SeqCst) {
                self.ping_all_hops();
            }

            if let Some(remaining) = interval.checked_sub(cycle_start.elapsed()) {
                std::thread::sleep(remaining);
            }
        }
    }

    fn ping_all_hops(&self) {
        // Collect all hops to ping, partitioned by mode
        let mut icmp_jobs: Vec<(String, i32, String, usize)> = Vec::new(); // (target, hop, ip, payload_size)
        let mut http_jobs: Vec<(String, String)> = Vec::new(); // (target_address, ip)
        {
            let targets = self.targets.lock().unwrap();
            for (address, state) in targets.iter() {
                for hop in &state.hops {
                    if hop.ip == "*" {
                        continue;
                    }
                    match state.probe_mode {
                        ProbeMode::Http => {
                            http_jobs.push((address.clone(), hop.ip.clone()));
                        }
                        _ => {
                            icmp_jobs.push((
                                address.clone(),
                                hop.hop,
                                hop.ip.clone(),
                                state.probe_mode.payload_size(),
                            ));
                        }
                    }
                }
            }
        }

        if icmp_jobs.is_empty() && http_jobs.is_empty() {
            return;
        }

        let mut records: Vec<PingRecord> = Vec::new();

        // ICMP jobs — worker pool
        if !icmp_jobs.is_empty() {
            let worker_count = usize::min(MAX_CONCURRENT_PINGS, icmp_jobs.len());
            let jobs = Arc::new(icmp_jobs);
            let next_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let records_shared = Arc::new(Mutex::new(Vec::<PingRecord>::with_capacity(jobs.len())));
            let mut workers = Vec::with_capacity(worker_count);

            for _ in 0..worker_count {
                let pinger = Arc::clone(&self.pinger);
                let jobs = Arc::clone(&jobs);
                let next_index = Arc::clone(&next_index);
                let records_shared = Arc::clone(&records_shared);
                workers.push(std::thread::spawn(move || loop {
                    let idx = next_index.fetch_add(1, Ordering::SeqCst);
                    if idx >= jobs.len() {
                        break;
                    }
                    let (target, hop, ip, payload_size) = &jobs[idx];
                    let timestamp = now_ms();
                    let latency = pinger.ping_host(ip, 1500, *payload_size);
                    let record = PingRecord {
                        timestamp,
                        target: target.clone(),
                        hop: *hop,
                        ip: ip.clone(),
                        latency_ms: latency,
                        is_timeout: latency.is_none(),
                    };
                    let mut out = records_shared.lock().unwrap();
                    out.push(record);
                }));
            }

            for worker in workers {
                let _ = worker.join();
            }

            let drained = {
                let mut out = records_shared.lock().unwrap();
                out.drain(..).collect::<Vec<_>>()
            };
            records.extend(drained);
        }

        // HTTP jobs — sequential (typically 1-3 targets)
        for (target_address, ip) in &http_jobs {
            let timestamp = now_ms();
            let latency = self.http_prober.probe(target_address);
            records.push(PingRecord {
                timestamp,
                target: target_address.clone(),
                hop: 1,
                ip: ip.clone(),
                latency_ms: latency,
                is_timeout: latency.is_none(),
            });
        }

        if !records.is_empty() {
            if let Ok(db) = self.db.lock() {
                if let Err(e) = db.record_ping_batch(&records) {
                    eprintln!("Failed to record pings: {}", e);
                }
            }
        }
    }
}
