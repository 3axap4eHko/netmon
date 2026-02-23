use std::sync::{Arc, Mutex};
use tauri::State;

use crate::db::Database;
use crate::mtr::MtrEngine;
use crate::ping::resolve_ipv4;
use crate::types::{DashboardData, HopStats, Target, TimeRange};

type DbState = Arc<Mutex<Database>>;
type EngineState = Arc<MtrEngine>;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tauri::command]
pub fn get_targets(db: State<'_, DbState>) -> Result<Vec<Target>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.get_targets().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_target(
    db: State<'_, DbState>,
    engine: State<'_, EngineState>,
    address: String,
    label: String,
) -> Result<Target, String> {
    if resolve_ipv4(&address).is_none() {
        return Err(
            "Target must be an IPv4 address or a hostname with a valid IPv4 DNS record."
                .to_string(),
        );
    }

    let target = {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.add_target(&address, &label).map_err(|e| e.to_string())?
    };
    engine.add_target(address);
    Ok(target)
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
    let since = now_ms() - range.duration_ms();
    let mut hops = db.get_live_stats(&target, since).map_err(|e| e.to_string())?;

    // Attach resolved hostnames from engine
    for hop in &mut hops {
        hop.hostname = engine.get_hostname(&hop.ip);
    }

    let loss_chart = db.get_loss_chart(&target, range).map_err(|e| e.to_string())?;
    let latency_chart = db.get_latency_chart(&target, range).map_err(|e| e.to_string())?;

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
    let since = now_ms() - 3_600_000; // 1 hour
    let mut hops = db.get_live_stats(&target, since).map_err(|e| e.to_string())?;
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
