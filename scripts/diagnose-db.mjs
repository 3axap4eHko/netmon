import { DatabaseSync } from 'node:sqlite';
import { join } from 'node:path';

function formatAge(ageMs) {
  if (ageMs < 1000) {
    return `${ageMs}ms`;
  }
  if (ageMs < 60_000) {
    return `${(ageMs / 1000).toFixed(1)}s`;
  }
  if (ageMs < 3_600_000) {
    return `${(ageMs / 60_000).toFixed(1)}m`;
  }
  return `${(ageMs / 3_600_000).toFixed(1)}h`;
}

const appData = process.env.APPDATA;
if (!appData) {
  console.error('APPDATA is not set.');
  process.exit(1);
}

const dbPath = join(appData, 'com.netmon.app', 'data.db');
const target = process.argv[2] ?? null;
const now = Date.now();
const oneHourAgo = now - 60 * 60 * 1000;

const db = new DatabaseSync(dbPath, { readOnly: true });

console.log(`db: ${dbPath}`);
console.log(`now: ${new Date(now).toISOString()}`);

if (target) {
  const latest = db.prepare(
    `SELECT MAX(timestamp) AS latest
     FROM pings
     WHERE target = ?`,
  ).get(target);
  const stats = db.prepare(
    `SELECT hop, ip, COUNT(*) AS sent,
            SUM(CASE WHEN is_timeout = 0 THEN 1 ELSE 0 END) AS recv,
            ROUND(MIN(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) AS best,
            ROUND(AVG(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) AS avg,
            ROUND(MAX(CASE WHEN is_timeout = 0 THEN latency_ms END), 1) AS worst
     FROM pings
     WHERE target = ?
       AND timestamp >= ?
       AND timestamp < ?
       AND ip != '*'
     GROUP BY hop, ip
     ORDER BY hop, ip`,
  ).all(target, oneHourAgo, now);

  if (!latest?.latest) {
    console.log(`target ${target}: no samples`);
    process.exit(0);
  }

  console.log(
    `target ${target}: latest sample ${new Date(latest.latest).toISOString()} (${formatAge(
      now - latest.latest,
    )} ago)`,
  );
  console.log(JSON.stringify(stats, null, 2));
  process.exit(0);
}

const rows = db.prepare(
  `SELECT target, MAX(timestamp) AS latest, COUNT(*) AS total
   FROM pings
   GROUP BY target
   ORDER BY target`,
).all();

for (const row of rows) {
  const ageMs = row.latest ? now - row.latest : null;
  console.log(
    `${row.target}: latest=${row.latest ? new Date(row.latest).toISOString() : 'never'} age=${
      ageMs === null ? 'n/a' : formatAge(ageMs)
    } total=${row.total}`,
  );
}
