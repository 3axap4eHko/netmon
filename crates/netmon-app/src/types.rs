use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const ICMP_PAYLOAD_STANDARD: usize = 32;
pub const ICMP_PAYLOAD_LARGE: usize = 1472; // 1500 MTU - 20 IP header - 8 ICMP header

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeMode {
    Icmp,      // 32-byte ICMP (default)
    IcmpLarge, // 1472-byte ICMP (MTU-sized)
    Http,      // HTTP POST (~12KB)
}

pub struct HttpEndpoint {
    pub key: &'static str,       // stored as target address in DB
    pub _label: &'static str,
    pub url: &'static str,
    pub payload_size: usize,
}

pub const HTTP_ENDPOINTS: &[HttpEndpoint] = &[
    HttpEndpoint {
        key: "cf-speed-12k",
        _label: "Cloudflare 12KB",
        url: "https://speed.cloudflare.com/__up",
        payload_size: 12_288,
    },
    HttpEndpoint {
        key: "cf-speed-100k",
        _label: "Cloudflare 100KB",
        url: "https://speed.cloudflare.com/__up",
        payload_size: 102_400,
    },
];

pub fn is_http_target_key(target: &str) -> bool {
    HTTP_ENDPOINTS.iter().any(|endpoint| endpoint.key == target)
}

impl Default for ProbeMode {
    fn default() -> Self {
        ProbeMode::Icmp
    }
}

impl ProbeMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "icmp-large" => ProbeMode::IcmpLarge,
            "http" => ProbeMode::Http,
            _ => ProbeMode::Icmp,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProbeMode::Icmp => "icmp",
            ProbeMode::IcmpLarge => "icmp-large",
            ProbeMode::Http => "http",
        }
    }

    pub fn payload_size(&self) -> usize {
        match self {
            ProbeMode::Icmp => ICMP_PAYLOAD_STANDARD,
            ProbeMode::IcmpLarge => ICMP_PAYLOAD_LARGE,
            ProbeMode::Http => 0, // not used for HTTP
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: i64,
    pub address: String,
    pub label: String,
    pub active: bool,
    pub probe_mode: ProbeMode,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeRange {
    OneHour,
    TwentyFourHours,
    SevenDays,
    ThirtyDays,
    CustomDay { timestamp: i64 },
}

impl Serialize for TimeRange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TimeRange::OneHour => serializer.serialize_str("1h"),
            TimeRange::TwentyFourHours => serializer.serialize_str("24h"),
            TimeRange::SevenDays => serializer.serialize_str("7d"),
            TimeRange::ThirtyDays => serializer.serialize_str("30d"),
            TimeRange::CustomDay { timestamp } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("customDay", timestamp)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for TimeRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de;

        struct TimeRangeVisitor;
        impl<'de> de::Visitor<'de> for TimeRangeVisitor {
            type Value = TimeRange;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a string like '1h' or an object like {{customDay: 123}}")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<TimeRange, E> {
                match v {
                    "1h" => Ok(TimeRange::OneHour),
                    "24h" => Ok(TimeRange::TwentyFourHours),
                    "7d" => Ok(TimeRange::SevenDays),
                    "30d" => Ok(TimeRange::ThirtyDays),
                    _ => Err(E::unknown_variant(v, &["1h", "24h", "7d", "30d"])),
                }
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<TimeRange, A::Error> {
                let key: String = map.next_key::<String>()?.ok_or_else(|| de::Error::custom("empty map"))?;
                if key == "customDay" {
                    let ts: i64 = map.next_value()?;
                    Ok(TimeRange::CustomDay { timestamp: ts })
                } else {
                    Err(de::Error::unknown_field(&key, &["customDay"]))
                }
            }
        }
        deserializer.deserialize_any(TimeRangeVisitor)
    }
}

impl TimeRange {
    pub fn duration_ms(&self) -> i64 {
        match self {
            TimeRange::OneHour => 60 * 60 * 1000,
            TimeRange::TwentyFourHours => 24 * 60 * 60 * 1000,
            TimeRange::SevenDays => 7 * 24 * 60 * 60 * 1000,
            TimeRange::ThirtyDays => 30 * 24 * 60 * 60 * 1000,
            TimeRange::CustomDay { .. } => 24 * 60 * 60 * 1000,
        }
    }

    pub fn bucket_ms(&self) -> i64 {
        match self {
            TimeRange::OneHour => 60 * 1000,          // 1 minute
            TimeRange::TwentyFourHours => 15 * 60 * 1000, // 15 minutes
            TimeRange::SevenDays => 60 * 60 * 1000,    // 1 hour
            TimeRange::ThirtyDays => 4 * 60 * 60 * 1000,  // 4 hours
            TimeRange::CustomDay { .. } => 60 * 60 * 1000, // 1 hour
        }
    }

    /// Which stats table to query for this range.
    /// Returns None for OneHour (uses raw pings table).
    pub fn stats_table(&self) -> Option<&'static str> {
        match self {
            TimeRange::OneHour => None,
            TimeRange::TwentyFourHours => Some("stats_15min"),
            TimeRange::SevenDays | TimeRange::ThirtyDays => Some("stats_hourly"),
            TimeRange::CustomDay { .. } => Some("stats_hourly"),
        }
    }

    /// Returns (since_ms, until_ms) for query window.
    pub fn query_window(&self) -> (i64, i64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        match self {
            TimeRange::CustomDay { timestamp } => (*timestamp, *timestamp + 24 * 60 * 60 * 1000),
            _ => (now - self.duration_ms(), now),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HopInfo {
    pub hop: i32,
    pub ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadTestResult {
    pub timestamp: i64,
    pub idle_latency_ms: f64,
    pub idle_jitter_ms: f64,
    pub download_mbps: f64,
    pub download_loaded_latency_ms: f64,
    pub upload_mbps: f64,
    pub upload_loaded_latency_ms: f64,
    pub grade: String,
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
