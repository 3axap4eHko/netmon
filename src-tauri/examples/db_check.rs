use rusqlite::Connection;

fn main() {
    let appdata = std::env::var("APPDATA").expect("No APPDATA");
    let db_path = format!("{}/com.netmon.app/data.db", appdata);
    println!("Opening: {}", db_path);

    let conn = Connection::open(&db_path).expect("Failed to open DB");
    conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

    let pings: i64 = conn
        .query_row("SELECT COUNT(*) FROM pings", [], |r| r.get(0))
        .unwrap();
    let (min_ts, max_ts): (Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT MIN(timestamp), MAX(timestamp) FROM pings",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let summaries: i64 = conn
        .query_row("SELECT COUNT(*) FROM ping_summaries", [], |r| r.get(0))
        .unwrap();
    let hourly: i64 = conn
        .query_row("SELECT COUNT(*) FROM ping_summaries_hourly", [], |r| {
            r.get(0)
        })
        .unwrap();
    let targets: Vec<(i64, String, String, i32)> = {
        let mut stmt = conn
            .prepare("SELECT id, address, label, active FROM targets")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    println!("\n=== Targets ===");
    for (id, addr, label, active) in &targets {
        println!("  id={} addr={} label={} active={}", id, addr, label, active);
    }

    println!("\n=== Raw pings ===");
    println!("  count: {}", pings);
    if let (Some(mn), Some(mx)) = (min_ts, max_ts) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        println!("  oldest: {}ms ago", now - mn);
        println!("  newest: {}ms ago", now - mx);
    }

    println!("\n=== Summaries (1-min) ===");
    println!("  count: {}", summaries);

    println!("\n=== Summaries (hourly) ===");
    println!("  count: {}", hourly);
}
