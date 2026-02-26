import { Hono } from 'hono';
import { cors } from './middleware/cors';
import { authMiddleware } from './middleware/auth';
import { authRoutes } from './routes/auth';
import { dataRoutes } from './routes/data';
import { deviceRoutes } from './routes/devices';
import { accountRoutes } from './routes/account';
import { webhookRoutes } from './routes/webhooks';

export interface Env {
  TURSO_URL: string;
  TURSO_AUTH_TOKEN: string;
  JWT_PRIVATE_KEY: string;
  JWT_PUBLIC_KEY: string;
  STRIPE_SECRET_KEY: string;
  STRIPE_WEBHOOK_SECRET: string;
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
  ENVIRONMENT: string;
}

const app = new Hono<{ Bindings: Env }>();

// Global middleware
app.use('*', cors());

// Public routes (no auth required)
app.route('/auth', authRoutes);
app.route('/webhooks', webhookRoutes);

// Protected routes
app.use('/data/*', authMiddleware());
app.use('/devices/*', authMiddleware());
app.use('/account/*', authMiddleware());

app.route('/data', dataRoutes);
app.route('/devices', deviceRoutes);
app.route('/account', accountRoutes);

// Health check
app.get('/health', (c) => c.json({ status: 'ok' }));

export default {
  fetch: app.fetch,

  async scheduled(event: ScheduledEvent, env: Env, ctx: ExecutionContext) {
    // Daily cleanup cron
    ctx.waitUntil(runRetentionCleanup(env));
  },
};

async function runRetentionCleanup(env: Env) {
  console.log('Running retention cleanup...');

  const { getSystemDb, getUserDb } = await import('./lib/turso');
  const sysDb = getSystemDb(env);

  // Get all users with per-user DBs
  const users = await sysDb.execute({
    sql: 'SELECT id, plan, turso_db_name FROM users WHERE turso_db_name IS NOT NULL',
    args: [],
  });

  const now = Date.now();

  for (const user of users.rows) {
    const plan = (user.plan as string) || 'free';
    const dbName = user.turso_db_name as string;
    const retentionMs = plan === 'pro' ? 30 * 24 * 60 * 60 * 1000 : 1 * 60 * 60 * 1000;
    const cutoff = now - retentionMs;
    const hourlyAggCutoff = now - 7 * 24 * 60 * 60 * 1000;

    try {
      const userDb = getUserDb(dbName, env.TURSO_AUTH_TOKEN);

      // Aggregate old 1-min summaries into hourly (for data older than 7 days)
      if (plan === 'pro') {
        const hourMs = 60 * 60 * 1000;
        await userDb.execute({
          sql: `INSERT INTO ping_summaries_hourly (timestamp, target, hop, ip, device_id, avg_latency, loss_pct, sample_count)
                SELECT (timestamp / ? * ?) as bucket_time, target, hop, ip, device_id,
                  ROUND(SUM(CASE WHEN avg_latency IS NOT NULL THEN avg_latency * sample_count ELSE 0 END)
                    / NULLIF(SUM(CASE WHEN avg_latency IS NOT NULL THEN sample_count ELSE 0 END), 0), 1),
                  ROUND(SUM(loss_pct * sample_count) / SUM(sample_count), 1),
                  SUM(sample_count)
                FROM ping_summaries
                WHERE timestamp < ?
                GROUP BY bucket_time, target, hop, ip, device_id`,
          args: [hourMs, hourMs, hourlyAggCutoff],
        });

        await userDb.execute({
          sql: 'DELETE FROM ping_summaries WHERE timestamp < ?',
          args: [hourlyAggCutoff],
        });
      }

      // Delete data beyond retention period
      await userDb.execute({
        sql: 'DELETE FROM ping_summaries WHERE timestamp < ?',
        args: [cutoff],
      });
      await userDb.execute({
        sql: 'DELETE FROM ping_summaries_hourly WHERE timestamp < ?',
        args: [cutoff],
      });

      console.log(`Cleaned up data for user ${user.id} (plan: ${plan})`);
    } catch (e) {
      console.error(`Failed to clean up user ${user.id}:`, e);
    }
  }

  // Deregister stale devices (90 days inactive)
  const staleDeviceCutoff = now - 90 * 24 * 60 * 60 * 1000;
  await sysDb.execute({
    sql: 'DELETE FROM devices WHERE last_push_at IS NOT NULL AND last_push_at < ?',
    args: [staleDeviceCutoff],
  });

  // Clean up expired auth codes
  const nowSecs = Math.floor(now / 1000);
  await sysDb.execute({
    sql: 'DELETE FROM auth_codes WHERE expires_at < ?',
    args: [nowSecs],
  });

  // Clean up expired refresh tokens
  await sysDb.execute({
    sql: 'DELETE FROM refresh_tokens WHERE expires_at < ?',
    args: [nowSecs],
  });

  console.log('Retention cleanup complete');
}
