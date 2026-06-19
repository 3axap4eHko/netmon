use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

use crate::db::Database;
use crate::load_test::LoadTestEngine;
use crate::mtr::MtrEngine;
use crate::ping::resolve_ipv4;
use crate::types::{
    is_http_target_key, DashboardData, HopStats, LoadTestResult, ProbeMode, Target, TimeRange,
};

type DbState = Arc<Mutex<Database>>;
type EngineState = Arc<MtrEngine>;

const UI_SETTINGS_FILE: &str = "ui_settings.json";

#[tauri::command]
pub fn get_ui_settings(app: tauri::AppHandle) -> Option<String> {
    let path = app.path().app_data_dir().ok()?.join(UI_SETTINGS_FILE);
    std::fs::read_to_string(path).ok()
}

#[tauri::command]
pub fn set_ui_settings(app: tauri::AppHandle, json: String) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(UI_SETTINGS_FILE), json).map_err(|e| e.to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn merge_engine_hops(
    engine: &Arc<MtrEngine>,
    target: &str,
    hops: &mut Vec<HopStats>,
) {
    let discovered = engine.get_target_hops(target);
    if discovered.is_empty() {
        return;
    }

    let mut by_key = std::collections::HashMap::new();
    for hop in hops.iter() {
        by_key.insert((hop.hop, hop.ip.clone()), true);
    }

    for hop in discovered {
        let key = (hop.hop, hop.ip.clone());
        if by_key.contains_key(&key) {
            continue;
        }

        hops.push(HopStats {
            hop: hop.hop,
            ip: hop.ip.clone(),
            hostname: engine.get_hostname(&hop.ip),
            loss_pct: 0.0,
            sent: 0,
            recv: 0,
            best: 0.0,
            avg: 0.0,
            worst: 0.0,
            last: 0.0,
        });
    }

    hops.sort_by(|a, b| a.hop.cmp(&b.hop).then_with(|| a.ip.cmp(&b.ip)));
}

#[tauri::command]
pub fn get_targets(db: State<'_, DbState>) -> Result<Vec<Target>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.get_active_targets().map_err(|e| e.to_string())
}

fn validate_target_probe_mode(address: &str, mode: ProbeMode) -> Result<(), String> {
    match mode {
        ProbeMode::Http => {
            if is_http_target_key(address) {
                Ok(())
            } else {
                Err(
                    "HTTP monitoring is only available for the built-in Cloudflare upload endpoints."
                        .to_string(),
                )
            }
        }
        _ => {
            if is_http_target_key(address) {
                return Err(
                    "Built-in HTTP upload endpoints cannot be switched to ICMP monitoring."
                        .to_string(),
                );
            }
            if resolve_ipv4(address).is_none() {
                return Err(
                    "Target must be an IPv4 address or a hostname with a valid IPv4 DNS record."
                        .to_string(),
                );
            }
            Ok(())
        }
    }
}

#[tauri::command]
pub fn add_target(
    db: State<'_, DbState>,
    engine: State<'_, EngineState>,
    address: String,
    label: String,
    probe_mode: Option<String>,
) -> Result<Target, String> {
    let mode = ProbeMode::from_str(&probe_mode.unwrap_or_default());
    validate_target_probe_mode(&address, mode)?;

    let target = {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.add_target(&address, &label, mode)
            .map_err(|e| e.to_string())?
    };
    engine.add_target(address, mode);
    Ok(target)
}

#[tauri::command]
pub fn set_probe_mode(
    db: State<'_, DbState>,
    engine: State<'_, EngineState>,
    address: String,
    probe_mode: String,
) -> Result<(), String> {
    let mode = ProbeMode::from_str(&probe_mode);
    validate_target_probe_mode(&address, mode)?;
    {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.update_probe_mode(&address, mode)
            .map_err(|e| e.to_string())?;
    }
    engine.set_probe_mode(&address, mode);
    Ok(())
}

#[tauri::command]
pub fn remove_target(
    db: State<'_, DbState>,
    engine: State<'_, EngineState>,
    id: i64,
) -> Result<(), String> {
    let address = {
        let db = db.lock().map_err(|e| e.to_string())?;
        let addr = db.get_target_address(id).map_err(|e| e.to_string())?;
        db.remove_target(id).map_err(|e| e.to_string())?;
        addr
    };
    if let Some(addr) = address {
        engine.remove_target(&addr);
    }
    Ok(())
}

#[tauri::command]
pub fn get_dashboard(
    db: State<'_, DbState>,
    engine: State<'_, EngineState>,
    target: String,
    range: TimeRange,
) -> Result<DashboardData, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let mut hops = db.get_live_stats(&target, range).map_err(|e| {
        let msg = format!(
            "get_live_stats failed for target={} range={:?}: {}",
            target, range, e
        );
        eprintln!("[dashboard] {}", msg);
        msg
    })?;
    merge_engine_hops(&engine, &target, &mut hops);

    // Attach resolved hostnames from engine
    for hop in &mut hops {
        hop.hostname = engine.get_hostname(&hop.ip);
    }

    let loss_chart = db.get_loss_chart(&target, range).map_err(|e| {
        let msg = format!(
            "get_loss_chart failed for target={} range={:?}: {}",
            target, range, e
        );
        eprintln!("[dashboard] {}", msg);
        msg
    })?;
    let latency_chart = db.get_latency_chart(&target, range).map_err(|e| {
        let msg = format!(
            "get_latency_chart failed for target={} range={:?}: {}",
            target, range, e
        );
        eprintln!("[dashboard] {}", msg);
        msg
    })?;

    Ok(DashboardData {
        target,
        hops,
        loss_chart,
        latency_chart,
    })
}

#[tauri::command]
pub fn get_live_stats(
    db: State<'_, DbState>,
    engine: State<'_, EngineState>,
    target: String,
) -> Result<Vec<HopStats>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let mut hops = db.get_live_stats(&target, TimeRange::OneHour).map_err(|e| {
        let msg = format!("get_live_stats failed for target={} range=OneHour: {}", target, e);
        eprintln!("[dashboard] {}", msg);
        msg
    })?;
    merge_engine_hops(&engine, &target, &mut hops);
    for hop in &mut hops {
        hop.hostname = engine.get_hostname(&hop.ip);
    }
    Ok(hops)
}

#[tauri::command]
pub fn pause_monitoring(engine: State<'_, EngineState>) -> Result<(), String> {
    engine.pause();
    Ok(())
}

#[tauri::command]
pub fn resume_monitoring(engine: State<'_, EngineState>) -> Result<(), String> {
    engine.resume();
    Ok(())
}

#[tauri::command]
pub fn get_monitoring_paused(engine: State<'_, EngineState>) -> Result<bool, String> {
    Ok(engine.is_paused())
}

#[tauri::command]
pub async fn run_load_test(engine: State<'_, Arc<LoadTestEngine>>) -> Result<LoadTestResult, String> {
    // run_test() spawns a dedicated OS thread internally and blocks on a
    // channel, keeping reqwest::blocking off the tokio runtime.
    engine.run_test()
}

#[tauri::command]
pub fn get_load_test_history(db: State<'_, DbState>) -> Result<Vec<LoadTestResult>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    // Return last 30 days of results
    let since = now_ms() - 30 * 24 * 60 * 60 * 1000;
    db.get_load_test_history(since).map_err(|e| e.to_string())
}
