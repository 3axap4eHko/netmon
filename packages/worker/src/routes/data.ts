import { Hono } from 'hono';
import type { Env } from '../index';
import { getSystemDb, getUserDb, initUserDbSchema } from '../lib/turso';

const data = new Hono<{ Bindings: Env }>();

// Push summaries from desktop app
data.post('/push', async (c) => {
  const user = c.get('user');

  const body = await c.req.json<{
    device_id: string;
    device_name: string;
    platform: string;
    targets: { address: string; label: string }[];
    summaries: {
      timestamp: number;
      target: string;
      hop: number;
      ip: string;
      avg_latency: number | null;
      loss_pct: number;
      sample_count: number;
    }[];
  }>();

  if (!body.summaries || body.summaries.length === 0) {
    return c.json({ ok: true, inserted: 0 });
  }

  // Rate limit check
  if (body.summaries.length > 1000) {
    return c.json({ error: 'Batch too large (max 1000)' }, 400);
  }

  const sysDb = getSystemDb(c.env);

  // Ensure user has a per-user DB
  let userResult = await sysDb.execute({
    sql: 'SELECT turso_db_name FROM users WHERE id = ?',
    args: [user.sub],
  });

  let dbName = userResult.rows[0]?.turso_db_name as string | null;
  if (!dbName) {
    dbName = `netmon-data-${user.sub}`;
    await sysDb.execute({
      sql: 'UPDATE users SET turso_db_name = ? WHERE id = ?',
      args: [dbName, user.sub],
    });
  }

  // Register/update device
  await sysDb.execute({
    sql: `INSERT INTO devices (id, user_id, name, platform, last_push_at)
          VALUES (?, ?, ?, ?, ?)
          ON CONFLICT(id) DO UPDATE SET last_push_at = ?, name = ?`,
    args: [body.device_id, user.sub, body.device_name, body.platform, Date.now(), Date.now(), body.device_name],
  });

  const userDb = getUserDb(dbName, c.env.TURSO_AUTH_TOKEN);
  await initUserDbSchema(userDb);

  // Upsert targets
  for (const t of body.targets) {
    await userDb.execute({
      sql: `INSERT INTO targets (address, label, device_id)
            VALUES (?, ?, ?)
            ON CONFLICT(address, device_id) DO UPDATE SET label = ?, active = 1`,
      args: [t.address, t.label, body.device_id, t.label],
    });
  }

  // Insert summaries in batch
  const batchStatements = body.summaries.map((s) => ({
    sql: `INSERT INTO ping_summaries (timestamp, target, hop, ip, device_id, avg_latency, loss_pct, sample_count)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    args: [s.timestamp, s.target, s.hop, s.ip, body.device_id, s.avg_latency, s.loss_pct, s.sample_count],
  }));

  await userDb.batch(batchStatements);

  return c.json({ ok: true, inserted: body.summaries.length });
});

// Get dashboard data for web
data.get('/dashboard', async (c) => {
  const user = c.get('user');
  const target = c.req.query('target');
  const range = c.req.query('range') || '1h';

  if (!target) {
    return c.json({ error: 'target required' }, 400);
  }

  const sysDb = getSystemDb(c.env);
  const userResult = await sysDb.execute({
    sql: 'SELECT turso_db_name FROM users WHERE id = ?',
    args: [user.sub],
  });

  const dbName = userResult.rows[0]?.turso_db_name as string | null;
  if (!dbName) {
    return c.json({ target, hops: [], lossChart: [], latencyChart: [] });
  }

  const userDb = getUserDb(dbName, c.env.TURSO_AUTH_TOKEN);

  const durationMs = rangeToDuration(range);
  const since = Date.now() - durationMs;
  const bucketMs = rangeToBucket(range);

  // Get hop stats
  const hopsResult = await userDb.execute({
    sql: `SELECT hop, ip,
            COUNT(*) as sent,
            SUM(CASE WHEN loss_pct < 100 THEN sample_count ELSE 0 END) as recv_samples,
            ROUND(AVG(loss_pct), 1) as avg_loss,
            ROUND(MIN(avg_latency), 1) as best,
            ROUND(AVG(avg_latency), 1) as avg,
            ROUND(MAX(avg_latency), 1) as worst
          FROM ping_summaries
          WHERE target = ? AND timestamp >= ? AND ip != '*'
          GROUP BY hop, ip ORDER BY hop`,
    args: [target, since],
  });

  const hops = hopsResult.rows.map((r) => ({
    hop: r.hop as number,
    ip: r.ip as string,
    hostname: null,
    lossPct: r.avg_loss as number,
    sent: r.sent as number,
    recv: r.recv_samples as number,
    best: (r.best as number) || 0,
    avg: (r.avg as number) || 0,
    worst: (r.worst as number) || 0,
    last: 0,
  }));

  // Get loss chart
  const lossResult = await userDb.execute({
    sql: `SELECT (timestamp / ? * ?) as bucket_time, hop,
            ROUND(SUM(loss_pct * sample_count / 100.0), 1) as loss_count
          FROM ping_summaries
          WHERE target = ? AND timestamp >= ? AND ip != '*'
          GROUP BY bucket_time, hop ORDER BY bucket_time, hop`,
    args: [bucketMs, bucketMs, target, since],
  });

  const lossChart = buildChartPoints(lossResult.rows);

  // Get latency chart
  const latencyResult = await userDb.execute({
    sql: `SELECT (timestamp / ? * ?) as bucket_time, hop,
            ROUND(AVG(avg_latency), 1) as avg_lat
          FROM ping_summaries
          WHERE target = ? AND timestamp >= ? AND ip != '*' AND avg_latency IS NOT NULL
          GROUP BY bucket_time, hop ORDER BY bucket_time, hop`,
    args: [bucketMs, bucketMs, target, since],
  });

  const latencyChart = buildChartPoints(latencyResult.rows);

  return c.json({ target, hops, lossChart, latencyChart });
});

// Get targets for the authenticated user
data.get('/targets', async (c) => {
  const user = c.get('user');

  const sysDb = getSystemDb(c.env);
  const userResult = await sysDb.execute({
    sql: 'SELECT turso_db_name FROM users WHERE id = ?',
    args: [user.sub],
  });

  const dbName = userResult.rows[0]?.turso_db_name as string | null;
  if (!dbName) {
    return c.json([]);
  }

  const userDb = getUserDb(dbName, c.env.TURSO_AUTH_TOKEN);
  const result = await userDb.execute({
    sql: 'SELECT id, address, label, active FROM targets WHERE active = 1',
    args: [],
  });

  return c.json(result.rows.map((r) => ({
    id: r.id as number,
    address: r.address as string,
    label: r.label as string,
    active: (r.active as number) === 1,
  })));
});

function rangeToDuration(range: string): number {
  switch (range) {
    case '24h': return 24 * 60 * 60 * 1000;
    case '7d': return 7 * 24 * 60 * 60 * 1000;
    case '30d': return 30 * 24 * 60 * 60 * 1000;
    default: return 60 * 60 * 1000;
  }
}

function rangeToBucket(range: string): number {
  switch (range) {
    case '24h': return 15 * 60 * 1000;
    case '7d': return 60 * 60 * 1000;
    case '30d': return 4 * 60 * 60 * 1000;
    default: return 60 * 1000;
  }
}

function buildChartPoints(rows: any[]): Record<string, number>[] {
  const map = new Map<number, Record<string, number>>();
  for (const row of rows) {
    const ts = row.bucket_time as number;
    const hop = row.hop as number;
    const value = (row.loss_count ?? row.avg_lat ?? 0) as number;
    if (!map.has(ts)) {
      map.set(ts, { timestamp: ts });
    }
    map.get(ts)![`hop${hop}`] = value;
  }
  return Array.from(map.values()).sort((a, b) => a.timestamp - b.timestamp);
}

export { data as dataRoutes };
