use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: i64,
    pub address: String,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HopStats {
    pub hop: i32,
    pub ip: String,
    pub hostname: Option<String>,
    pub loss_pct: f64,
    pub sent: i64,
    pub recv: i64,
    pub best: f64,
    pub avg: f64,
    pub worst: f64,
    pub last: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartPoint {
    pub timestamp: i64,
    #[serde(flatten)]
    pub hops: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardData {
    pub target: String,
    pub hops: Vec<HopStats>,
    pub loss_chart: Vec<ChartPoint>,
    pub latency_chart: Vec<ChartPoint>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimeRange {
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "24h")]
    TwentyFourHours,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
}

impl TimeRange {
    pub fn duration_ms(&self) -> i64 {
        match self {
            TimeRange::OneHour => 60 * 60 * 1000,
            TimeRange::TwentyFourHours => 24 * 60 * 60 * 1000,
            TimeRange::SevenDays => 7 * 24 * 60 * 60 * 1000,
            TimeRange::ThirtyDays => 30 * 24 * 60 * 60 * 1000,
        }
    }

    pub fn bucket_ms(&self) -> i64 {
        match self {
            TimeRange::OneHour => 60 * 1000,          // 1 minute
            TimeRange::TwentyFourHours => 15 * 60 * 1000, // 15 minutes
            TimeRange::SevenDays => 60 * 60 * 1000,    // 1 hour
            TimeRange::ThirtyDays => 4 * 60 * 60 * 1000,  // 4 hours
        }
    }
}

#[derive(Debug, Clone)]
pub struct HopInfo {
    pub hop: i32,
    pub ip: String,
}

#[derive(Debug, Clone)]
pub struct PingRecord {
    pub timestamp: i64,
    pub target: String,
    pub hop: i32,
    pub ip: String,
    pub latency_ms: Option<f64>,
    pub is_timeout: bool,
}
