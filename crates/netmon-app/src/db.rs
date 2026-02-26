use rusqlite::{params, Connection, Result as SqlResult};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::DeviceInfo;
use crate::types::{ChartPoint, HopStats, LoadTestResult, Target, TimeRange, PingRecord};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(db_path: &Path) -> SqlResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(db_path)?;

        // WAL mode + performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-8000;
             PRAGMA temp_store=MEMORY;",
        )?;

        // Create schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS targets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL UNIQUE,
                label TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS pings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                latency_ms REAL,
                is_timeout INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS ping_summaries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                avg_latency REAL,
                loss_pct REAL NOT NULL,
                sample_count INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ping_summaries_hourly (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                avg_latency REAL,
                loss_pct REAL NOT NULL,
                sample_count INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pings_timestamp ON pings(timestamp);
            CREATE INDEX IF NOT EXISTS idx_pings_target ON pings(target, hop);
            CREATE INDEX IF NOT EXISTS idx_summaries_timestamp ON ping_summaries(timestamp);
            CREATE INDEX IF NOT EXISTS idx_summaries_target ON ping_summaries(target, hop);
            CREATE INDEX IF NOT EXISTS idx_summaries_hourly_timestamp ON ping_summaries_hourly(timestamp);
            CREATE INDEX IF NOT EXISTS idx_summaries_hourly_target ON ping_summaries_hourly(target, hop);

            CREATE TABLE IF NOT EXISTS device_info (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                platform TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS auth_tokens (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                user_id TEXT NOT NULL,
                email TEXT NOT NULL,
                plan TEXT NOT NULL,
                access_token TEXT NOT NULL,
                refresh_token TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sync_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_push_timestamp INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS load_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                idle_latency REAL NOT NULL,
                idle_jitter REAL NOT NULL,
                download_mbps REAL NOT NULL,
                download_loaded_latency REAL NOT NULL,
                upload_mbps REAL NOT NULL,
                upload_loaded_latency REAL NOT NULL,
                grade TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_load_tests_ts ON load_tests(timestamp);",
        )?;

        let db = Database { conn };

        // Seed default target if empty
        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM targets", [], |r| r.get(0))?;
        if count == 0 {
            db.conn.execute(
                "INSERT INTO targets (address, label, active) VALUES (?1, ?2, 1)",
                params!["8.8.8.8", "Google DNS"],
            )?;
        }

        Ok(db)
    }

    /// Migrate an existing Electron database (adds missing tables).
    pub fn migrate_from_electron(&self) -> SqlResult<()> {
        // The ping_summaries_hourly table is new - if we imported an Electron DB it won't exist
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ping_summaries_hourly (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                avg_latency REAL,
                loss_pct REAL NOT NULL,
                sample_count INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_summaries_hourly_timestamp ON ping_summaries_hourly(timestamp);
            CREATE INDEX IF NOT EXISTS idx_summaries_hourly_target ON ping_summaries_hourly(target, hop);",
        )?;
        Ok(())
    }

    // -- Target queries --

    pub fn get_targets(&self) -> SqlResult<Vec<Target>> {
        let mut stmt = self.conn.prepare("SELECT id, address, label, active FROM targets")?;
        let rows = stmt.query_map([], |row| {
            Ok(Target {
                id: row.get(0)?,
                address: row.get(1)?,
                label: row.get(2)?,
                active: row.get::<_, i32>(3)? != 0,
            })
        })?;
        rows.collect()
    }

    pub fn get_active_targets(&self) -> SqlResult<Vec<Target>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, address, label, active FROM targets WHERE active = 1")?;
        let rows = stmt.query_map([], |row| {
            Ok(Target {
                id: row.get(0)?,
                address: row.get(1)?,
                label: row.get(2)?,
                active: true,
            })
        })?;
        rows.collect()
    }

    pub fn add_target(&self, address: &str, label: &str) -> SqlResult<Target> {
        // Upsert: if exists, reactivate and update label
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM targets WHERE address = ?1",
                params![address],
                |r| r.get(0),
            )
            .ok();

        if let Some(_id) = existing {
            self.conn.execute(
                "UPDATE targets SET active = 1, label = ?1 WHERE address = ?2",
                params![label, address],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO targets (address, label, active) VALUES (?1, ?2, 1)",
                params![address, label],
            )?;
        }

        self.conn.query_row(
            "SELECT id, address, label, active FROM targets WHERE address = ?1",
            params![address],
            |row| {
                Ok(Target {
                    id: row.get(0)?,
                    address: row.get(1)?,
                    label: row.get(2)?,
                    active: row.get::<_, i32>(3)? != 0,
                })
            },
        )
    }

    pub fn remove_target(&self, id: i64) -> SqlResult<()> {
        self.conn
            .execute("UPDATE targets SET active = 0 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_target_address(&self, id: i64) -> SqlResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT address FROM targets WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
    }

    // -- Ping recording --

    pub fn record_ping_batch(&self, records: &[PingRecord]) -> SqlResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO pings (timestamp, target, hop, ip, latency_ms, is_timeout) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for r in records {
                stmt.execute(params![
                    r.timestamp,
                    r.target,
                    r.hop,
                    r.ip,
                    r.latency_ms,
                    r.is_timeout as i32
                ])?;
            }
        }
        tx.commit()
    }

    // -- Stats queries --

    pub fn get_live_stats(&self, target: &str, since_timestamp: i64) -> SqlResult<Vec<HopStats>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                hop, ip,
                COUNT(*) as sent,
                SUM(CASE WHEN is_timeout = 0 THEN 1 ELSE 0 END) as recv,
                ROUND(100.0 * SUM(is_timeout) / COUNT(*), 1) as loss_pct,
                ROUND(MIN(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as best,
                ROUND(AVG(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as avg,
                ROUND(MAX(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as worst
            FROM pings
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY hop, ip
            ORDER BY hop",
        )?;

        let stats: Vec<HopStats> = stmt
            .query_map(params![target, since_timestamp], |row| {
                Ok(HopStats {
                    hop: row.get(0)?,
                    ip: row.get(1)?,
                    hostname: None,
                    loss_pct: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                    sent: row.get(2)?,
                    recv: row.get(3)?,
                    best: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                    avg: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                    worst: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                    last: 0.0,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;

        // Get last ping for each hop
        let mut last_stmt = self.conn.prepare(
            "SELECT hop, ip, latency_ms, is_timeout
            FROM pings
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            AND id IN (
                SELECT MAX(id) FROM pings WHERE target = ?1 AND timestamp >= ?2 AND ip != '*' GROUP BY hop, ip
            )
            ORDER BY hop",
        )?;
        let mut last_map: HashMap<(i32, String), f64> = HashMap::new();
        let last_rows = last_stmt.query_map(params![target, since_timestamp], |row| {
            let hop: i32 = row.get(0)?;
            let ip: String = row.get(1)?;
            let latency: Option<f64> = row.get(2)?;
            let is_timeout: i32 = row.get(3)?;
            Ok((hop, ip, latency, is_timeout))
        })?;
        for row in last_rows {
            let (hop, ip, latency, is_timeout) = row?;
            if is_timeout != 0 {
                last_map.insert((hop, ip), -1.0);
            } else {
                last_map.insert((hop, ip), latency.unwrap_or(-1.0));
            }
        }

        Ok(stats
            .into_iter()
            .map(|mut s| {
                s.last = *last_map
                    .get(&(s.hop, s.ip.clone()))
                    .unwrap_or(&0.0);
                s
            })
            .collect())
    }

    // -- Chart queries with downsampling --

    pub fn get_loss_chart(&self, target: &str, range: TimeRange) -> SqlResult<Vec<ChartPoint>> {
        let now = now_ms();
        let since = now - range.duration_ms();
        let bucket = range.bucket_ms();

        let mut points = self.query_loss_from_pings(target, since, bucket)?;

        // Raw pings are pruned after 2h, so any range beyond 1h needs summary data
        if range != TimeRange::OneHour {
            let summary_points = self.query_loss_from_summaries(target, since, bucket)?;
            points = merge_chart_points(summary_points, points);
        }
        if range == TimeRange::SevenDays || range == TimeRange::ThirtyDays {
            let hourly_points = self.query_loss_from_hourly(target, since, bucket)?;
            points = merge_chart_points(hourly_points, points);
        }

        Ok(downsample(points, 200))
    }

    pub fn get_latency_chart(&self, target: &str, range: TimeRange) -> SqlResult<Vec<ChartPoint>> {
        let now = now_ms();
        let since = now - range.duration_ms();
        let bucket = range.bucket_ms();

        let mut points = self.query_latency_from_pings(target, since, bucket)?;

        if range != TimeRange::OneHour {
            let summary_points = self.query_latency_from_summaries(target, since, bucket)?;
            points = merge_chart_points(summary_points, points);
        }
        if range == TimeRange::SevenDays || range == TimeRange::ThirtyDays {
            let hourly_points = self.query_latency_from_hourly(target, since, bucket)?;
            points = merge_chart_points(hourly_points, points);
        }

        Ok(downsample(points, 200))
    }

    fn query_loss_from_pings(
        &self,
        target: &str,
        since: i64,
        bucket: i64,
    ) -> SqlResult<Vec<ChartPoint>> {
        let sql = format!(
            "SELECT
                (timestamp / {bucket} * {bucket}) as bucket_time,
                hop,
                SUM(is_timeout) as loss_count
            FROM pings
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY bucket_time, hop
            ORDER BY bucket_time, hop"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![target, since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        Ok(build_chart_points(rows.collect::<SqlResult<Vec<_>>>()?))
    }

    fn query_loss_from_summaries(
        &self,
        target: &str,
        since: i64,
        bucket: i64,
    ) -> SqlResult<Vec<ChartPoint>> {
        let sql = format!(
            "SELECT
                (timestamp / {bucket} * {bucket}) as bucket_time,
                hop,
                ROUND(SUM(loss_pct * sample_count / 100.0), 1) as loss_count
            FROM ping_summaries
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY bucket_time, hop
            ORDER BY bucket_time, hop"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![target, since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        Ok(build_chart_points(rows.collect::<SqlResult<Vec<_>>>()?))
    }

    fn query_loss_from_hourly(
        &self,
        target: &str,
        since: i64,
        bucket: i64,
    ) -> SqlResult<Vec<ChartPoint>> {
        let sql = format!(
            "SELECT
                (timestamp / {bucket} * {bucket}) as bucket_time,
                hop,
                ROUND(SUM(loss_pct * sample_count / 100.0), 1) as loss_count
            FROM ping_summaries_hourly
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY bucket_time, hop
            ORDER BY bucket_time, hop"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![target, since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        Ok(build_chart_points(rows.collect::<SqlResult<Vec<_>>>()?))
    }

    fn query_latency_from_pings(
        &self,
        target: &str,
        since: i64,
        bucket: i64,
    ) -> SqlResult<Vec<ChartPoint>> {
        let sql = format!(
            "SELECT
                (timestamp / {bucket} * {bucket}) as bucket_time,
                hop,
                ROUND(AVG(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as avg_latency
            FROM pings
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY bucket_time, hop
            ORDER BY bucket_time, hop"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![target, since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?;
        let tuples: Vec<(i64, i32, Option<f64>)> = rows.collect::<SqlResult<Vec<_>>>()?;
        // Filter out null latencies
        let filtered: Vec<(i64, i32, f64)> = tuples
            .into_iter()
            .filter_map(|(ts, hop, lat)| lat.map(|l| (ts, hop, l)))
            .collect();
        Ok(build_chart_points(filtered))
    }

    fn query_latency_from_summaries(
        &self,
        target: &str,
        since: i64,
        bucket: i64,
    ) -> SqlResult<Vec<ChartPoint>> {
        let sql = format!(
            "SELECT
                (timestamp / {bucket} * {bucket}) as bucket_time,
                hop,
                ROUND(
                    SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count ELSE 0 END)
                    / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count ELSE 0 END), 0),
                    1
                ) as avg_latency
            FROM ping_summaries
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY bucket_time, hop
            ORDER BY bucket_time, hop"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![target, since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?;
        let tuples: Vec<(i64, i32, Option<f64>)> = rows.collect::<SqlResult<Vec<_>>>()?;
        let filtered: Vec<(i64, i32, f64)> = tuples
            .into_iter()
            .filter_map(|(ts, hop, lat)| lat.map(|l| (ts, hop, l)))
            .collect();
        Ok(build_chart_points(filtered))
    }

    fn query_latency_from_hourly(
        &self,
        target: &str,
        since: i64,
        bucket: i64,
    ) -> SqlResult<Vec<ChartPoint>> {
        let sql = format!(
            "SELECT
                (timestamp / {bucket} * {bucket}) as bucket_time,
                hop,
                ROUND(
                    SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count ELSE 0 END)
                    / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count ELSE 0 END), 0),
                    1
                ) as avg_latency
            FROM ping_summaries_hourly
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            GROUP BY bucket_time, hop
            ORDER BY bucket_time, hop"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![target, since], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?;
        let tuples: Vec<(i64, i32, Option<f64>)> = rows.collect::<SqlResult<Vec<_>>>()?;
        let filtered: Vec<(i64, i32, f64)> = tuples
            .into_iter()
            .filter_map(|(ts, hop, lat)| lat.map(|l| (ts, hop, l)))
            .collect();
        Ok(build_chart_points(filtered))
    }

    // -- Aggregation (continuous, bounded) --

    pub fn run_aggregation(&self) -> SqlResult<()> {
        let now = now_ms();
        let two_hours_ago = now - 2 * 60 * 60 * 1000;
        let seven_days_ago = now - 7 * 24 * 60 * 60 * 1000;
        let thirty_days_ago = now - 30 * 24 * 60 * 60 * 1000;
        let minute_ms: i64 = 60 * 1000;
        let hour_ms: i64 = 60 * 60 * 1000;

        // 1. Aggregate raw pings older than 2 hours into 1-minute summaries
        let sql1 = format!(
            "SELECT
                (timestamp / {minute_ms} * {minute_ms}) as bucket_time,
                target, hop, ip,
                ROUND(AVG(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as avg_latency,
                ROUND(100.0 * SUM(is_timeout) / COUNT(*), 1) as loss_pct,
                COUNT(*) as sample_count
            FROM pings
            WHERE timestamp < ?1
            GROUP BY bucket_time, target, hop, ip"
        );
        let old_pings = query_aggregation_rows(&self.conn, &sql1, two_hours_ago)?;

        if !old_pings.is_empty() {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare_cached(
                    "INSERT INTO ping_summaries (timestamp, target, hop, ip, avg_latency, loss_pct, sample_count)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for p in &old_pings {
                    insert.execute(params![p.0, p.1, p.2, p.3, p.4, p.5, p.6])?;
                }
            }
            tx.execute("DELETE FROM pings WHERE timestamp < ?1", params![two_hours_ago])?;
            tx.commit()?;
        }

        // 2. Aggregate 1-minute summaries older than 7 days into hourly summaries
        let sql2 = format!(
            "SELECT
                (timestamp / {hour_ms} * {hour_ms}) as bucket_time,
                target, hop, ip,
                ROUND(
                    SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count ELSE 0 END)
                    / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count ELSE 0 END), 0),
                    1
                ) as avg_latency,
                ROUND(SUM(loss_pct * sample_count) / SUM(sample_count), 1) as loss_pct,
                SUM(sample_count) as sample_count
            FROM ping_summaries
            WHERE timestamp < ?1
            GROUP BY bucket_time, target, hop, ip"
        );
        let old_summaries = query_aggregation_rows(&self.conn, &sql2, seven_days_ago)?;

        if !old_summaries.is_empty() {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare_cached(
                    "INSERT INTO ping_summaries_hourly (timestamp, target, hop, ip, avg_latency, loss_pct, sample_count)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for p in &old_summaries {
                    insert.execute(params![p.0, p.1, p.2, p.3, p.4, p.5, p.6])?;
                }
            }
            tx.execute(
                "DELETE FROM ping_summaries WHERE timestamp < ?1",
                params![seven_days_ago],
            )?;
            tx.commit()?;
        }

        // 3. Delete hourly summaries older than 30 days
        self.conn.execute(
            "DELETE FROM ping_summaries_hourly WHERE timestamp < ?1",
            params![thirty_days_ago],
        )?;

        Ok(())
    }

    // -- Load test results --

    pub fn record_load_test(&self, result: &LoadTestResult) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO load_tests (timestamp, idle_latency, idle_jitter, download_mbps, download_loaded_latency, upload_mbps, upload_loaded_latency, grade)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                result.timestamp,
                result.idle_latency_ms,
                result.idle_jitter_ms,
                result.download_mbps,
                result.download_loaded_latency_ms,
                result.upload_mbps,
                result.upload_loaded_latency_ms,
                result.grade,
            ],
        )?;
        Ok(())
    }

    pub fn get_load_test_history(&self, since: i64) -> SqlResult<Vec<LoadTestResult>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, idle_latency, idle_jitter, download_mbps, download_loaded_latency, upload_mbps, upload_loaded_latency, grade
             FROM load_tests
             WHERE timestamp >= ?1
             ORDER BY timestamp DESC
             LIMIT 20",
        )?;
        let rows = stmt.query_map(params![since], |row| {
            Ok(LoadTestResult {
                timestamp: row.get(0)?,
                idle_latency_ms: row.get(1)?,
                idle_jitter_ms: row.get(2)?,
                download_mbps: row.get(3)?,
                download_loaded_latency_ms: row.get(4)?,
                upload_mbps: row.get(5)?,
                upload_loaded_latency_ms: row.get(6)?,
                grade: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    // -- Device info --

    pub fn get_or_create_device_info(&self) -> DeviceInfo {
        let result = self.conn.query_row(
            "SELECT device_id, device_name, platform FROM device_info WHERE id = 1",
            [],
            |row| {
                Ok(DeviceInfo {
                    device_id: row.get(0)?,
                    device_name: row.get(1)?,
                    platform: row.get(2)?,
                })
            },
        );

        match result {
            Ok(info) => info,
            Err(_) => {
                let device_id = format!("dev_{}", uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string());
                let device_name = hostname::get()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "Unknown".to_string());
                let platform = std::env::consts::OS.to_string();

                self.conn
                    .execute(
                        "INSERT OR REPLACE INTO device_info (id, device_id, device_name, platform) VALUES (1, ?1, ?2, ?3)",
                        params![device_id, device_name, platform],
                    )
                    .ok();

                DeviceInfo {
                    device_id,
                    device_name,
                    platform,
                }
            }
        }
    }

    // -- Auth tokens --

    pub fn get_auth_tokens(&self) -> Option<AuthTokens> {
        self.conn
            .query_row(
                "SELECT user_id, email, plan, access_token, refresh_token, expires_at FROM auth_tokens WHERE id = 1",
                [],
                |row| {
                    Ok(AuthTokens {
                        user_id: row.get(0)?,
                        email: row.get(1)?,
                        plan: row.get(2)?,
                        access_token: row.get(3)?,
                        refresh_token: row.get(4)?,
                        expires_at: row.get(5)?,
                    })
                },
            )
            .ok()
    }

    pub fn store_auth_tokens(
        &self,
        user_id: &str,
        email: &str,
        plan: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: i64,
    ) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO auth_tokens (id, user_id, email, plan, access_token, refresh_token, expires_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![user_id, email, plan, access_token, refresh_token, expires_at],
        )?;
        Ok(())
    }

    pub fn update_access_token(&self, access_token: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE auth_tokens SET access_token = ?1 WHERE id = 1",
            params![access_token],
        )?;
        Ok(())
    }

    pub fn clear_auth_tokens(&self) -> SqlResult<()> {
        self.conn.execute("DELETE FROM auth_tokens", [])?;
        Ok(())
    }

    // -- Sync state --

    pub fn get_sync_watermark(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT last_push_timestamp FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    pub fn set_sync_watermark(&self, timestamp: i64) {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO sync_state (id, last_push_timestamp) VALUES (1, ?1)",
                params![timestamp],
            )
            .ok();
    }

    pub fn get_summaries_since(
        &self,
        since_timestamp: i64,
    ) -> SqlResult<Vec<(i64, String, i32, String, Option<f64>, f64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, target, hop, ip, avg_latency, loss_pct, sample_count
             FROM ping_summaries
             WHERE timestamp > ?1
             ORDER BY timestamp
             LIMIT 1000",
        )?;
        let rows = stmt.query_map(params![since_timestamp], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?;
        rows.collect()
    }
}

pub struct AuthTokens {
    pub user_id: String,
    pub email: String,
    pub plan: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

// -- Helper functions --

fn build_chart_points(rows: Vec<(i64, i32, f64)>) -> Vec<ChartPoint> {
    let mut map: HashMap<i64, ChartPoint> = HashMap::new();
    for (ts, hop, value) in rows {
        let point = map.entry(ts).or_insert_with(|| ChartPoint {
            timestamp: ts,
            hops: HashMap::new(),
        });
        point.hops.insert(format!("hop{}", hop), value);
    }
    let mut points: Vec<ChartPoint> = map.into_values().collect();
    points.sort_by_key(|p| p.timestamp);
    points
}

fn merge_chart_points(base: Vec<ChartPoint>, overlay: Vec<ChartPoint>) -> Vec<ChartPoint> {
    let mut map: HashMap<i64, ChartPoint> = HashMap::new();
    for p in base {
        map.insert(p.timestamp, p);
    }
    // Overlay (raw data) takes priority
    for p in overlay {
        let entry = map.entry(p.timestamp).or_insert_with(|| ChartPoint {
            timestamp: p.timestamp,
            hops: HashMap::new(),
        });
        for (k, v) in p.hops {
            entry.hops.insert(k, v);
        }
    }
    let mut points: Vec<ChartPoint> = map.into_values().collect();
    points.sort_by_key(|p| p.timestamp);
    points
}

fn query_aggregation_rows(
    conn: &Connection,
    sql: &str,
    since: i64,
) -> SqlResult<Vec<(i64, String, i32, String, Option<f64>, f64, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![since])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ));
    }
    Ok(results)
}

fn downsample(points: Vec<ChartPoint>, max_points: usize) -> Vec<ChartPoint> {
    if points.len() <= max_points {
        return points;
    }
    let step = points.len() as f64 / max_points as f64;
    let mut result = Vec::with_capacity(max_points);
    let mut i = 0.0;
    while (i as usize) < points.len() && result.len() < max_points {
        result.push(points[i as usize].clone());
        i += step;
    }
    // Always include the last point
    if let Some(last) = points.last() {
        if result.last().map(|r| r.timestamp) != Some(last.timestamp) {
            result.push(last.clone());
        }
    }
    result
}
