use rusqlite::{params, Connection, Result as SqlResult};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::DeviceInfo;
use crate::types::{ChartPoint, HopStats, LoadTestResult, ProbeMode, Target, TimeRange, PingRecord};

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
            CREATE INDEX IF NOT EXISTS idx_pings_timestamp ON pings(timestamp);
            CREATE INDEX IF NOT EXISTS idx_pings_target ON pings(target, hop);
            CREATE INDEX IF NOT EXISTS idx_pings_target_ts ON pings(target, timestamp);

            CREATE TABLE IF NOT EXISTS stats_15min (
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                sent INTEGER NOT NULL,
                recv INTEGER NOT NULL,
                loss_count INTEGER NOT NULL,
                avg_latency REAL,
                min_latency REAL,
                max_latency REAL,
                UNIQUE(timestamp, target, hop, ip)
            );
            CREATE INDEX IF NOT EXISTS idx_stats_15min_lookup ON stats_15min(target, timestamp);

            CREATE TABLE IF NOT EXISTS stats_hourly (
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                sent INTEGER NOT NULL,
                recv INTEGER NOT NULL,
                loss_count INTEGER NOT NULL,
                avg_latency REAL,
                min_latency REAL,
                max_latency REAL,
                UNIQUE(timestamp, target, hop, ip)
            );
            CREATE INDEX IF NOT EXISTS idx_stats_hourly_lookup ON stats_hourly(target, timestamp);

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

        // Migrate: add probe_mode column (silently skip if already exists)
        conn.execute(
            "ALTER TABLE targets ADD COLUMN probe_mode TEXT NOT NULL DEFAULT 'icmp'",
            [],
        )
        .ok();

        let db = Database { conn };

        // Migrate data from old tables if they exist
        db.migrate_old_tables()?;

        // Seed all preset targets (upsert — won't overwrite existing)
        db.seed_presets()?;

        Ok(db)
    }

    /// One-time migration: old ping_summaries/ping_summaries_hourly → new stats tables
    fn migrate_old_tables(&self) -> SqlResult<()> {
        // Check if old tables exist
        let old_exists: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='ping_summaries'",
            [],
            |row| row.get::<_, i64>(0),
        )? > 0;

        if !old_exists {
            return Ok(());
        }

        eprintln!("[db] Migrating old ping_summaries → stats_15min...");

        let tx = self.conn.unchecked_transaction()?;

        // ping_summaries (1-min buckets) → stats_15min (15-min buckets)
        tx.execute_batch(
            "INSERT OR REPLACE INTO stats_15min (timestamp, target, hop, ip, sent, recv, loss_count, avg_latency, min_latency, max_latency)
             SELECT
                 (timestamp / 900000 * 900000) as bucket_time,
                 target, hop, ip,
                 SUM(sample_count) as sent,
                 CAST(ROUND(SUM(sample_count * (100.0 - loss_pct) / 100.0)) AS INTEGER) as recv,
                 CAST(ROUND(SUM(sample_count * loss_pct / 100.0)) AS INTEGER) as loss_count,
                 ROUND(
                     SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END)
                     / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END), 0),
                     1
                 ) as avg_latency,
                 ROUND(
                     SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END)
                     / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END), 0),
                     1
                 ) as min_latency,
                 ROUND(
                     SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END)
                     / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END), 0),
                     1
                 ) as max_latency
             FROM ping_summaries
             GROUP BY bucket_time, target, hop, ip;"
        )?;

        // Check if hourly table exists too
        let hourly_exists: bool = tx.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='ping_summaries_hourly'",
            [],
            |row| row.get::<_, i64>(0),
        )? > 0;

        if hourly_exists {
            eprintln!("[db] Migrating old ping_summaries_hourly → stats_hourly...");
            tx.execute_batch(
                "INSERT OR REPLACE INTO stats_hourly (timestamp, target, hop, ip, sent, recv, loss_count, avg_latency, min_latency, max_latency)
                 SELECT
                     (timestamp / 3600000 * 3600000) as bucket_time,
                     target, hop, ip,
                     SUM(sample_count) as sent,
                     CAST(ROUND(SUM(sample_count * (100.0 - loss_pct) / 100.0)) AS INTEGER) as recv,
                     CAST(ROUND(SUM(sample_count * loss_pct / 100.0)) AS INTEGER) as loss_count,
                     ROUND(
                         SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END)
                         / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END), 0),
                         1
                     ) as avg_latency,
                     ROUND(
                         SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END)
                         / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END), 0),
                         1
                     ) as min_latency,
                     ROUND(
                         SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END)
                         / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count * (100.0 - loss_pct) / 100.0 ELSE 0 END), 0),
                         1
                     ) as max_latency
                 FROM ping_summaries_hourly
                 GROUP BY bucket_time, target, hop, ip;"
            )?;
            tx.execute_batch("DROP TABLE ping_summaries_hourly;")?;
        }

        tx.execute_batch("DROP TABLE ping_summaries;")?;

        // Drop old indexes that reference dropped tables
        tx.execute_batch(
            "DROP INDEX IF EXISTS idx_summaries_timestamp;
             DROP INDEX IF EXISTS idx_summaries_target;
             DROP INDEX IF EXISTS idx_summaries_hourly_timestamp;
             DROP INDEX IF EXISTS idx_summaries_hourly_target;"
        )?;

        tx.commit()?;
        eprintln!("[db] Migration complete — old tables dropped.");
        Ok(())
    }

    fn seed_presets(&self) -> SqlResult<()> {
        let presets: &[(&str, &str, &str)] = &[
            ("1.1.1.1", "Cloudflare", "icmp"),
            ("8.8.8.8", "Google DNS", "icmp"),
            ("9.9.9.9", "Quad9", "icmp"),
            ("208.67.222.222", "OpenDNS", "icmp"),
            ("cf-speed-12k", "Cloudflare 12KB", "http"),
            ("cf-speed-100k", "Cloudflare 100KB", "http"),
        ];
        for (address, label, mode) in presets {
            self.conn.execute(
                "INSERT OR IGNORE INTO targets (address, label, active, probe_mode) VALUES (?1, ?2, 1, ?3)",
                params![address, label, mode],
            )?;
        }
        Ok(())
    }

    /// Migrate an existing Electron database (adds missing tables).
    pub fn migrate_from_electron(&self) -> SqlResult<()> {
        // The new stats tables should already exist from open(), nothing extra needed
        Ok(())
    }

    // -- Target queries --

    pub fn get_active_targets(&self) -> SqlResult<Vec<Target>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, address, label, active, probe_mode FROM targets WHERE active = 1 ORDER BY id",
            )?;
        let rows = stmt.query_map([], |row| {
            Ok(Target {
                id: row.get(0)?,
                address: row.get(1)?,
                label: row.get(2)?,
                active: true,
                probe_mode: ProbeMode::from_str(&row.get::<_, String>(4).unwrap_or_default()),
            })
        })?;
        rows.collect()
    }

    pub fn add_target(&self, address: &str, label: &str, probe_mode: ProbeMode) -> SqlResult<Target> {
        // Upsert: if exists, reactivate and update label + probe_mode
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
                "UPDATE targets SET active = 1, label = ?1, probe_mode = ?2 WHERE address = ?3",
                params![label, probe_mode.as_str(), address],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO targets (address, label, active, probe_mode) VALUES (?1, ?2, 1, ?3)",
                params![address, label, probe_mode.as_str()],
            )?;
        }

        self.conn.query_row(
            "SELECT id, address, label, active, probe_mode FROM targets WHERE address = ?1",
            params![address],
            |row| {
                Ok(Target {
                    id: row.get(0)?,
                    address: row.get(1)?,
                    label: row.get(2)?,
                    active: row.get::<_, i32>(3)? != 0,
                    probe_mode: ProbeMode::from_str(&row.get::<_, String>(4).unwrap_or_default()),
                })
            },
        )
    }

    pub fn update_probe_mode(&self, address: &str, probe_mode: ProbeMode) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE targets SET probe_mode = ?1 WHERE address = ?2",
            params![probe_mode.as_str(), address],
        )?;
        Ok(())
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
    //
    // Each range queries non-overlapping time slices across tiers via UNION ALL:
    //   stats_hourly  covers [since, current_hour)
    //   stats_15min   covers [current_hour, current_15min)  (gap not yet in hourly)
    //   pings         covers [current_15min, until)          (gap not yet in 15min)
    // For 24h: skip stats_hourly tier. For 1h: pings only.

    /// Tier boundary timestamps for splitting queries across tables.
    fn tier_boundaries(&self) -> (i64, i64) {
        let now = now_ms();
        let fifteen_min_ms: i64 = 15 * 60 * 1000;
        let hour_ms: i64 = 60 * 60 * 1000;
        let current_hour = (now / hour_ms) * hour_ms;
        let current_15min = (now / fifteen_min_ms) * fifteen_min_ms;
        (current_hour, current_15min)
    }

    pub fn get_live_stats(&self, target: &str, range: TimeRange) -> SqlResult<Vec<HopStats>> {
        let (since, until) = range.query_window();
        let (current_hour, current_15min) = self.tier_boundaries();

        // Build a UNION ALL query across non-overlapping tiers
        let sql = match range.stats_table() {
            Some("stats_hourly") => {
                // 7d / 30d / CustomDay: all three tiers
                format!(
                    "SELECT hop, ip, SUM(sent), SUM(recv), ROUND(MIN(best),1), \
                        ROUND(SUM(CASE WHEN avg IS NOT NULL THEN avg*recv ELSE 0 END) \
                        / NULLIF(SUM(CASE WHEN avg IS NOT NULL THEN recv ELSE 0 END),0),1), \
                        ROUND(MAX(worst),1) \
                    FROM ( \
                        SELECT hop, ip, sent, recv, min_latency as best, avg_latency as avg, max_latency as worst \
                        FROM stats_hourly WHERE target=?1 AND timestamp>=?2 AND timestamp<?3 AND ip!='*' \
                        UNION ALL \
                        SELECT hop, ip, sent, recv, min_latency, avg_latency, max_latency \
                        FROM stats_15min WHERE target=?1 AND timestamp>=?4 AND timestamp<?5 AND ip!='*' \
                        UNION ALL \
                        SELECT hop, ip, COUNT(*), SUM(CASE WHEN is_timeout=0 THEN 1 ELSE 0 END), \
                            MIN(CASE WHEN is_timeout=0 THEN latency_ms END), \
                            AVG(CASE WHEN is_timeout=0 THEN latency_ms END), \
                            MAX(CASE WHEN is_timeout=0 THEN latency_ms END) \
                        FROM pings WHERE target=?1 AND timestamp>=?6 AND timestamp<?3 AND ip!='*' \
                        GROUP BY hop, ip \
                    ) t GROUP BY hop, ip ORDER BY hop",
                    // ?1=target ?2=since ?3=until ?4=current_hour ?5=current_15min ?6=current_15min
                )
            }
            Some("stats_15min") => {
                // 24h: stats_15min + raw pings for gap
                format!(
                    "SELECT hop, ip, SUM(sent), SUM(recv), ROUND(MIN(best),1), \
                        ROUND(SUM(CASE WHEN avg IS NOT NULL THEN avg*recv ELSE 0 END) \
                        / NULLIF(SUM(CASE WHEN avg IS NOT NULL THEN recv ELSE 0 END),0),1), \
                        ROUND(MAX(worst),1) \
                    FROM ( \
                        SELECT hop, ip, sent, recv, min_latency as best, avg_latency as avg, max_latency as worst \
                        FROM stats_15min WHERE target=?1 AND timestamp>=?2 AND timestamp<?5 AND ip!='*' \
                        UNION ALL \
                        SELECT hop, ip, COUNT(*), SUM(CASE WHEN is_timeout=0 THEN 1 ELSE 0 END), \
                            MIN(CASE WHEN is_timeout=0 THEN latency_ms END), \
                            AVG(CASE WHEN is_timeout=0 THEN latency_ms END), \
                            MAX(CASE WHEN is_timeout=0 THEN latency_ms END) \
                        FROM pings WHERE target=?1 AND timestamp>=?6 AND timestamp<?3 AND ip!='*' \
                        GROUP BY hop, ip \
                    ) t GROUP BY hop, ip ORDER BY hop"
                )
            }
            _ => {
                // 1h: raw pings only
                "SELECT hop, ip, COUNT(*), SUM(CASE WHEN is_timeout=0 THEN 1 ELSE 0 END), \
                    ROUND(MIN(CASE WHEN is_timeout=0 THEN latency_ms END),1), \
                    ROUND(AVG(CASE WHEN is_timeout=0 THEN latency_ms END),1), \
                    ROUND(MAX(CASE WHEN is_timeout=0 THEN latency_ms END),1) \
                FROM pings WHERE target=?1 AND timestamp>=?2 AND timestamp<?3 AND ip!='*' \
                GROUP BY hop, ip ORDER BY hop".to_string()
            }
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let mut stats_map: HashMap<(i32, String), HopStats> = HashMap::new();
        let rows: Vec<HopStats> = match range.stats_table() {
            Some(_) => stmt
                .query_map(
                    params![target, since, until, current_hour, current_15min, current_15min],
                    |row| {
                        Ok(HopStats {
                            hop: row.get(0)?,
                            ip: row.get(1)?,
                            hostname: None,
                            loss_pct: 0.0,
                            sent: row.get(2)?,
                            recv: row.get(3)?,
                            best: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                            avg: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                            worst: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                            last: 0.0,
                        })
                    },
                )?
                .collect::<SqlResult<Vec<_>>>()?,
            None => stmt
                .query_map(params![target, since, until], |row| {
                    Ok(HopStats {
                        hop: row.get(0)?,
                        ip: row.get(1)?,
                        hostname: None,
                        loss_pct: 0.0,
                        sent: row.get(2)?,
                        recv: row.get(3)?,
                        best: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
                        avg: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
                        worst: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
                        last: 0.0,
                    })
                })?
                .collect::<SqlResult<Vec<_>>>()?,
        };
        for s in rows {
            stats_map.insert((s.hop, s.ip.clone()), s);
        }

        // Recalculate loss_pct
        let mut stats: Vec<HopStats> = stats_map
            .into_values()
            .map(|mut s| {
                s.loss_pct = if s.sent > 0 {
                    let raw = 100.0 * (s.sent - s.recv) as f64 / s.sent as f64;
                    (raw * 10.0).round() / 10.0
                } else {
                    0.0
                };
                s
            })
            .collect();
        stats.sort_by(|a, b| a.hop.cmp(&b.hop).then_with(|| a.ip.cmp(&b.ip)));

        // Get last ping for each hop (from raw pings — always recent)
        let mut last_stmt = self.conn.prepare(
            "SELECT hop, ip, latency_ms, is_timeout
            FROM pings
            WHERE target = ?1 AND timestamp >= ?2 AND ip != '*'
            AND id IN (
                SELECT MAX(id) FROM pings WHERE target = ?1 AND timestamp >= ?2 AND ip != '*' GROUP BY hop, ip
            )
            ORDER BY hop",
        )?;
        let last_since = now_ms() - 2 * 60 * 60 * 1000;
        let mut last_map: HashMap<(i32, String), f64> = HashMap::new();
        let last_rows = last_stmt.query_map(params![target, last_since], |row| {
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

    // -- Chart queries (tiered UNION ALL, same non-overlapping slices) --

    pub fn get_loss_chart(&self, target: &str, range: TimeRange) -> SqlResult<Vec<ChartPoint>> {
        let (since, until) = range.query_window();
        let (current_hour, current_15min) = self.tier_boundaries();
        let bucket = range.bucket_ms();

        let sql = match range.stats_table() {
            Some("stats_hourly") => format!(
                "SELECT bucket_time, hop, SUM(loss) FROM ( \
                    SELECT (timestamp/{bucket}*{bucket}) as bucket_time, hop, SUM(loss_count) as loss \
                    FROM stats_hourly WHERE target=?1 AND timestamp>=?2 AND timestamp<?3 AND ip!='*' \
                    GROUP BY bucket_time, hop \
                    UNION ALL \
                    SELECT (timestamp/{bucket}*{bucket}), hop, SUM(loss_count) \
                    FROM stats_15min WHERE target=?1 AND timestamp>=?4 AND timestamp<?5 AND ip!='*' \
                    GROUP BY 1, hop \
                    UNION ALL \
                    SELECT (timestamp/{bucket}*{bucket}), hop, SUM(is_timeout) \
                    FROM pings WHERE target=?1 AND timestamp>=?6 AND timestamp<?3 AND ip!='*' \
                    GROUP BY 1, hop \
                ) t GROUP BY bucket_time, hop ORDER BY bucket_time, hop"
            ),
            Some("stats_15min") => format!(
                "SELECT bucket_time, hop, SUM(loss) FROM ( \
                    SELECT (timestamp/{bucket}*{bucket}) as bucket_time, hop, SUM(loss_count) as loss \
                    FROM stats_15min WHERE target=?1 AND timestamp>=?2 AND timestamp<?5 AND ip!='*' \
                    GROUP BY bucket_time, hop \
                    UNION ALL \
                    SELECT (timestamp/{bucket}*{bucket}), hop, SUM(is_timeout) \
                    FROM pings WHERE target=?1 AND timestamp>=?6 AND timestamp<?3 AND ip!='*' \
                    GROUP BY 1, hop \
                ) t GROUP BY bucket_time, hop ORDER BY bucket_time, hop"
            ),
            _ => format!(
                "SELECT (timestamp/{bucket}*{bucket}) as bucket_time, hop, SUM(is_timeout) as loss \
                FROM pings WHERE target=?1 AND timestamp>=?2 AND timestamp<?3 AND ip!='*' \
                GROUP BY bucket_time, hop ORDER BY bucket_time, hop"
            ),
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<(i64, i32, f64)> = match range.stats_table() {
            Some(_) => stmt
                .query_map(
                    params![target, since, until, current_hour, current_15min, current_15min],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i32>(1)?, row.get::<_, f64>(2)?)),
                )?
                .collect::<SqlResult<Vec<_>>>()?,
            None => stmt
                .query_map(params![target, since, until], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i32>(1)?, row.get::<_, f64>(2)?))
                })?
                .collect::<SqlResult<Vec<_>>>()?,
        };
        Ok(downsample(build_chart_points(rows), 200))
    }

    pub fn get_latency_chart(&self, target: &str, range: TimeRange) -> SqlResult<Vec<ChartPoint>> {
        let (since, until) = range.query_window();
        let (current_hour, current_15min) = self.tier_boundaries();
        let bucket = range.bucket_ms();

        let sql = match range.stats_table() {
            Some("stats_hourly") => format!(
                "SELECT bucket_time, hop, \
                    ROUND(SUM(lat_sum)/NULLIF(SUM(lat_weight),0),1) \
                FROM ( \
                    SELECT (timestamp/{bucket}*{bucket}) as bucket_time, hop, \
                        SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency*recv ELSE 0 END) as lat_sum, \
                        SUM(CASE WHEN avg_latency IS NOT NULL THEN recv ELSE 0 END) as lat_weight \
                    FROM stats_hourly WHERE target=?1 AND timestamp>=?2 AND timestamp<?3 AND ip!='*' \
                    GROUP BY bucket_time, hop \
                    UNION ALL \
                    SELECT (timestamp/{bucket}*{bucket}), hop, \
                        SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency*recv ELSE 0 END), \
                        SUM(CASE WHEN avg_latency IS NOT NULL THEN recv ELSE 0 END) \
                    FROM stats_15min WHERE target=?1 AND timestamp>=?4 AND timestamp<?5 AND ip!='*' \
                    GROUP BY 1, hop \
                    UNION ALL \
                    SELECT (timestamp/{bucket}*{bucket}), hop, \
                        SUM(CASE WHEN is_timeout=0 THEN latency_ms ELSE 0 END), \
                        SUM(CASE WHEN is_timeout=0 THEN 1 ELSE 0 END) \
                    FROM pings WHERE target=?1 AND timestamp>=?6 AND timestamp<?3 AND ip!='*' \
                    GROUP BY 1, hop \
                ) t GROUP BY bucket_time, hop ORDER BY bucket_time, hop"
            ),
            Some("stats_15min") => format!(
                "SELECT bucket_time, hop, \
                    ROUND(SUM(lat_sum)/NULLIF(SUM(lat_weight),0),1) \
                FROM ( \
                    SELECT (timestamp/{bucket}*{bucket}) as bucket_time, hop, \
                        SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency*recv ELSE 0 END) as lat_sum, \
                        SUM(CASE WHEN avg_latency IS NOT NULL THEN recv ELSE 0 END) as lat_weight \
                    FROM stats_15min WHERE target=?1 AND timestamp>=?2 AND timestamp<?5 AND ip!='*' \
                    GROUP BY bucket_time, hop \
                    UNION ALL \
                    SELECT (timestamp/{bucket}*{bucket}), hop, \
                        SUM(CASE WHEN is_timeout=0 THEN latency_ms ELSE 0 END), \
                        SUM(CASE WHEN is_timeout=0 THEN 1 ELSE 0 END) \
                    FROM pings WHERE target=?1 AND timestamp>=?6 AND timestamp<?3 AND ip!='*' \
                    GROUP BY 1, hop \
                ) t GROUP BY bucket_time, hop ORDER BY bucket_time, hop"
            ),
            _ => format!(
                "SELECT (timestamp/{bucket}*{bucket}) as bucket_time, hop, \
                    ROUND(AVG(CASE WHEN is_timeout=0 THEN latency_ms END),1) \
                FROM pings WHERE target=?1 AND timestamp>=?2 AND timestamp<?3 AND ip!='*' \
                GROUP BY bucket_time, hop ORDER BY bucket_time, hop"
            ),
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let tuples: Vec<(i64, i32, Option<f64>)> = match range.stats_table() {
            Some(_) => stmt
                .query_map(
                    params![target, since, until, current_hour, current_15min, current_15min],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i32>(1)?,
                            row.get::<_, Option<f64>>(2)?,
                        ))
                    },
                )?
                .collect::<SqlResult<Vec<_>>>()?,
            None => stmt
                .query_map(params![target, since, until], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, Option<f64>>(2)?,
                    ))
                })?
                .collect::<SqlResult<Vec<_>>>()?,
        };
        let filtered: Vec<(i64, i32, f64)> = tuples
            .into_iter()
            .filter_map(|(ts, hop, lat)| lat.map(|l| (ts, hop, l)))
            .collect();
        Ok(downsample(build_chart_points(filtered), 200))
    }

    // -- Aggregation (continuous, bounded) --
    //
    // Key principle: aggregate based on BUCKET COMPLETION (not retention).
    // Each tier gets data as soon as the bucket closes, then prune separately.

    pub fn run_aggregation(&self) -> SqlResult<()> {
        let now = now_ms();
        let fifteen_min_ms: i64 = 15 * 60 * 1000;
        let hour_ms: i64 = 60 * 60 * 1000;

        // Bucket completion boundaries (current incomplete bucket excluded)
        let current_15min_start = (now / fifteen_min_ms) * fifteen_min_ms;
        let current_hour_start = (now / hour_ms) * hour_ms;

        // Retention boundaries
        let two_hours_ago = now - 2 * 60 * 60 * 1000;
        let forty_eight_hours_ago = now - 48 * 60 * 60 * 1000;
        let thirty_days_ago = now - 30 * 24 * 60 * 60 * 1000;

        // 1. pings → stats_15min: aggregate ALL completed 15-min buckets
        let sql1 = format!(
            "SELECT
                (timestamp / {fifteen_min_ms} * {fifteen_min_ms}) as bucket_time,
                target, hop, ip,
                COUNT(*) as sent,
                SUM(CASE WHEN is_timeout = 0 THEN 1 ELSE 0 END) as recv,
                SUM(is_timeout) as loss_count,
                ROUND(AVG(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as avg_latency,
                ROUND(MIN(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as min_latency,
                ROUND(MAX(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) as max_latency
            FROM pings
            WHERE timestamp < ?1
            GROUP BY bucket_time, target, hop, ip"
        );
        let mut stmt = self.conn.prepare(&sql1)?;
        let mut rows = stmt.query(params![current_15min_start])?;
        let mut agg_rows: Vec<(i64, String, i32, String, i64, i64, i64, Option<f64>, Option<f64>, Option<f64>)> = Vec::new();
        while let Some(row) = rows.next()? {
            agg_rows.push((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                row.get(4)?, row.get(5)?, row.get(6)?,
                row.get(7)?, row.get(8)?, row.get(9)?,
            ));
        }
        drop(rows);
        drop(stmt);

        if !agg_rows.is_empty() {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare_cached(
                    "INSERT OR REPLACE INTO stats_15min (timestamp, target, hop, ip, sent, recv, loss_count, avg_latency, min_latency, max_latency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )?;
                for r in &agg_rows {
                    insert.execute(params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9])?;
                }
            }
            // Only delete pings older than 2h (retention), keep recent for 1h view
            tx.execute("DELETE FROM pings WHERE timestamp < ?1", params![two_hours_ago])?;
            tx.commit()?;
        }

        // 2. stats_15min → stats_hourly: aggregate ALL completed hourly buckets
        let sql2 = format!(
            "SELECT
                (timestamp / {hour_ms} * {hour_ms}) as bucket_time,
                target, hop, ip,
                SUM(sent) as sent,
                SUM(recv) as recv,
                SUM(loss_count) as loss_count,
                ROUND(
                    SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * recv ELSE 0 END)
                    / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN recv ELSE 0 END), 0),
                    1
                ) as avg_latency,
                ROUND(MIN(min_latency), 1) as min_latency,
                ROUND(MAX(max_latency), 1) as max_latency
            FROM stats_15min
            WHERE timestamp < ?1
            GROUP BY bucket_time, target, hop, ip"
        );
        let mut stmt2 = self.conn.prepare(&sql2)?;
        let mut rows2 = stmt2.query(params![current_hour_start])?;
        let mut hourly_rows: Vec<(i64, String, i32, String, i64, i64, i64, Option<f64>, Option<f64>, Option<f64>)> = Vec::new();
        while let Some(row) = rows2.next()? {
            hourly_rows.push((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                row.get(4)?, row.get(5)?, row.get(6)?,
                row.get(7)?, row.get(8)?, row.get(9)?,
            ));
        }
        drop(rows2);
        drop(stmt2);

        if !hourly_rows.is_empty() {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare_cached(
                    "INSERT OR REPLACE INTO stats_hourly (timestamp, target, hop, ip, sent, recv, loss_count, avg_latency, min_latency, max_latency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )?;
                for r in &hourly_rows {
                    insert.execute(params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9])?;
                }
            }
            // Only delete stats_15min older than 48h (retention), keep recent for 24h view
            tx.execute("DELETE FROM stats_15min WHERE timestamp < ?1", params![forty_eight_hours_ago])?;
            tx.commit()?;
        }

        // 3. Prune: stats_hourly > 30d
        self.conn.execute(
            "DELETE FROM stats_hourly WHERE timestamp < ?1",
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

    /// Returns (timestamp, target, hop, ip, avg_latency, loss_pct, sample_count)
    /// for sync compatibility — computed from stats_15min's sent/recv columns.
    pub fn get_summaries_since(
        &self,
        since_timestamp: i64,
    ) -> SqlResult<Vec<(i64, String, i32, String, Option<f64>, f64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, target, hop, ip, avg_latency, sent, recv
             FROM stats_15min
             WHERE timestamp > ?1
             ORDER BY timestamp
             LIMIT 1000",
        )?;
        let rows = stmt.query_map(params![since_timestamp], |row| {
            let sent: i64 = row.get(5)?;
            let recv: i64 = row.get(6)?;
            let loss_pct = if sent > 0 {
                ((sent - recv) as f64 / sent as f64 * 1000.0).round() / 10.0
            } else {
                0.0
            };
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                loss_pct,
                sent,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE pings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                target TEXT NOT NULL,
                hop INTEGER NOT NULL,
                ip TEXT NOT NULL,
                latency_ms REAL,
                is_timeout INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        Database { conn }
    }

    #[test]
    fn one_hour_queries_use_raw_pings_without_extra_bind_params() {
        let db = make_test_db();
        let timestamp = now_ms() - 60_000;

        db.record_ping_batch(&[
            PingRecord {
                timestamp,
                target: "1.1.1.1".to_string(),
                hop: 1,
                ip: "1.1.1.1".to_string(),
                latency_ms: Some(12.5),
                is_timeout: false,
            },
            PingRecord {
                timestamp: timestamp + 1_000,
                target: "1.1.1.1".to_string(),
                hop: 1,
                ip: "1.1.1.1".to_string(),
                latency_ms: None,
                is_timeout: true,
            },
        ])
        .unwrap();

        let stats = db.get_live_stats("1.1.1.1", TimeRange::OneHour).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].sent, 2);
        assert_eq!(stats[0].recv, 1);

        let loss_chart = db.get_loss_chart("1.1.1.1", TimeRange::OneHour).unwrap();
        assert!(!loss_chart.is_empty());

        let latency_chart = db.get_latency_chart("1.1.1.1", TimeRange::OneHour).unwrap();
        assert!(!latency_chart.is_empty());
    }
}
