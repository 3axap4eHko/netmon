use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use crate::db::Database;
use crate::types::{LoadTestResult, ProbeMode, TimeRange};

const OUTAGE_LOSS_THRESHOLD: f64 = 5.0;
const ATTRIBUTION_MIN_LOSS: f64 = 1.0;
const HOUR_MS: i64 = 60 * 60 * 1000;
const PROBE_INTERVAL_SECS: f64 = 2.0;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HopAttribution {
    pub hop: i32,
    pub ip: String,
    pub loss_pct: f64,
    pub scope: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetReport {
    pub address: String,
    pub label: String,
    pub probe_mode: ProbeMode,
    pub samples: i64,
    pub loss_pct: f64,
    pub avg_latency_ms: f64,
    pub worst_latency_ms: f64,
    pub availability_pct: f64,
    pub first_loss_hop: Option<HopAttribution>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutageEvent {
    pub start: i64,
    pub end: i64,
    pub duration_secs: i64,
    pub peak_loss_pct: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportBucket {
    pub timestamp: i64,
    pub sent: i64,
    pub loss_pct: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportData {
    pub generated_at: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub device_name: String,
    pub platform: String,
    pub overall_loss_pct: f64,
    pub total_samples: i64,
    pub targets: Vec<TargetReport>,
    pub outages: Vec<OutageEvent>,
    pub loss_series: Vec<ReportBucket>,
    pub load_tests: Vec<LoadTestResult>,
    pub probe_interval_secs: f64,
}

fn make_outage(start: i64, last_bucket: i64, bucket_ms: i64, peak: f64) -> OutageEvent {
    let end = last_bucket + bucket_ms;
    OutageEvent {
        start,
        end,
        duration_secs: (end - start) / 1000,
        peak_loss_pct: round1(peak),
    }
}

/// Detect runs of contiguous hourly buckets whose loss exceeds the threshold.
/// A gap in the series (missing hour) breaks a run so outages are not overstated.
fn detect_outages(series: &[ReportBucket], bucket_ms: i64) -> Vec<OutageEvent> {
    let mut outages = Vec::new();
    let mut run: Option<(i64, i64, f64)> = None; // (start, last_bucket, peak)

    for bucket in series {
        let high = bucket.loss_pct >= OUTAGE_LOSS_THRESHOLD;
        if let Some((start, last_bucket, peak)) = run {
            let contiguous = bucket.timestamp == last_bucket + bucket_ms;
            if high && contiguous {
                run = Some((start, bucket.timestamp, peak.max(bucket.loss_pct)));
                continue;
            }
            outages.push(make_outage(start, last_bucket, bucket_ms, peak));
            run = None;
        }
        if high {
            run = Some((bucket.timestamp, bucket.timestamp, bucket.loss_pct));
        }
    }

    if let Some((start, last_bucket, peak)) = run {
        outages.push(make_outage(start, last_bucket, bucket_ms, peak));
    }
    outages
}

#[tauri::command]
pub fn generate_report(
    db: State<'_, Arc<Mutex<Database>>>,
    range: TimeRange,
    targets: Option<Vec<String>>,
) -> Result<ReportData, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let device = db.get_or_create_device_info();
    let (since, until) = range.query_window();

    let active = db.get_active_targets().map_err(|e| e.to_string())?;
    let selected = match &targets {
        Some(list) => active
            .into_iter()
            .filter(|t| list.contains(&t.address))
            .collect::<Vec<_>>(),
        None => active,
    };

    let mut target_reports = Vec::with_capacity(selected.len());
    let mut combined: BTreeMap<i64, (i64, i64)> = BTreeMap::new();

    for target in &selected {
        let hops = db
            .get_live_stats(&target.address, range)
            .map_err(|e| e.to_string())?;

        let Some(dest) = hops.iter().max_by_key(|h| h.hop).cloned() else {
            target_reports.push(TargetReport {
                address: target.address.clone(),
                label: target.label.clone(),
                probe_mode: target.probe_mode,
                samples: 0,
                loss_pct: 0.0,
                avg_latency_ms: 0.0,
                worst_latency_ms: 0.0,
                availability_pct: 0.0,
                first_loss_hop: None,
            });
            continue;
        };

        let first_loss_hop = if dest.loss_pct >= ATTRIBUTION_MIN_LOSS {
            let threshold = (dest.loss_pct * 0.5).max(2.0);
            let mut sorted: Vec<_> = hops.iter().collect();
            sorted.sort_by_key(|h| h.hop);
            sorted
                .into_iter()
                .find(|h| h.loss_pct >= threshold)
                .map(|h| HopAttribution {
                    hop: h.hop,
                    ip: h.ip.clone(),
                    loss_pct: h.loss_pct,
                    scope: if h.hop <= 1 {
                        "local-gateway".to_string()
                    } else {
                        "beyond-gateway".to_string()
                    },
                })
        } else {
            None
        };

        let series = db
            .get_hop_loss_series(&target.address, dest.hop, since, until)
            .map_err(|e| e.to_string())?;
        for (bucket, sent, loss) in series {
            let entry = combined.entry(bucket).or_insert((0, 0));
            entry.0 += sent;
            entry.1 += loss;
        }

        target_reports.push(TargetReport {
            address: target.address.clone(),
            label: target.label.clone(),
            probe_mode: target.probe_mode,
            samples: dest.sent,
            loss_pct: dest.loss_pct,
            avg_latency_ms: dest.avg,
            worst_latency_ms: dest.worst,
            availability_pct: round1((100.0 - dest.loss_pct).max(0.0)),
            first_loss_hop,
        });
    }

    let mut total_sent = 0i64;
    let mut total_loss = 0i64;
    let loss_series: Vec<ReportBucket> = combined
        .into_iter()
        .map(|(timestamp, (sent, loss))| {
            total_sent += sent;
            total_loss += loss;
            let loss_pct = if sent > 0 {
                round1(loss as f64 / sent as f64 * 100.0)
            } else {
                0.0
            };
            ReportBucket {
                timestamp,
                sent,
                loss_pct,
            }
        })
        .collect();

    let overall_loss_pct = if total_sent > 0 {
        round1(total_loss as f64 / total_sent as f64 * 100.0)
    } else {
        0.0
    };

    let outages = detect_outages(&loss_series, HOUR_MS);

    let load_tests = db
        .get_load_test_history(since)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|lt| lt.timestamp <= until)
        .collect();

    Ok(ReportData {
        generated_at: now_ms(),
        period_start: since,
        period_end: until,
        device_name: device.device_name,
        platform: device.platform,
        overall_loss_pct,
        total_samples: total_sent,
        targets: target_reports,
        outages,
        loss_series,
        load_tests,
        probe_interval_secs: PROBE_INTERVAL_SECS,
    })
}
