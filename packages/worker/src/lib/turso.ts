import { createClient, Client } from '@libsql/client';

let systemClient: Client | null = null;

export function getSystemDb(env: { TURSO_URL: string; TURSO_AUTH_TOKEN: string }): Client {
  if (!systemClient) {
    systemClient = createClient({
      url: env.TURSO_URL,
      authToken: env.TURSO_AUTH_TOKEN,
    });
  }
  return systemClient;
}

export function getUserDb(dbName: string, authToken: string): Client {
  return createClient({
    url: `libsql://${dbName}-netmon.turso.io`,
    authToken,
  });
}

export async function initSystemSchema(db: Client): Promise<void> {
  await db.executeMultiple(`
    CREATE TABLE IF NOT EXISTS users (
      id TEXT PRIMARY KEY,
      email TEXT NOT NULL UNIQUE,
      password_hash TEXT,
      plan TEXT NOT NULL DEFAULT 'free',
      stripe_customer_id TEXT,
      turso_db_name TEXT,
      created_at INTEGER NOT NULL DEFAULT (unixepoch())
    );

    CREATE TABLE IF NOT EXISTS oauth_identities (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      user_id TEXT NOT NULL REFERENCES users(id),
      provider TEXT NOT NULL,
      provider_user_id TEXT NOT NULL,
      UNIQUE(provider, provider_user_id)
    );

    CREATE TABLE IF NOT EXISTS devices (
      id TEXT PRIMARY KEY,
      user_id TEXT NOT NULL REFERENCES users(id),
      name TEXT NOT NULL,
      platform TEXT NOT NULL,
      last_push_at INTEGER,
      created_at INTEGER NOT NULL DEFAULT (unixepoch())
    );

    CREATE TABLE IF NOT EXISTS refresh_tokens (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      user_id TEXT NOT NULL REFERENCES users(id),
      device_id TEXT NOT NULL,
      token_hash TEXT NOT NULL UNIQUE,
      expires_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS auth_codes (
      code TEXT PRIMARY KEY,
      user_id TEXT NOT NULL REFERENCES users(id),
      code_challenge TEXT NOT NULL,
      device_id TEXT NOT NULL,
      expires_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS subscriptions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      user_id TEXT NOT NULL REFERENCES users(id),
      stripe_subscription_id TEXT NOT NULL UNIQUE,
      status TEXT NOT NULL,
      current_period_end INTEGER NOT NULL
    );
  `);
}

export async function initUserDbSchema(db: Client): Promise<void> {
  await db.executeMultiple(`
    CREATE TABLE IF NOT EXISTS targets (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      address TEXT NOT NULL,
      label TEXT NOT NULL,
      device_id TEXT NOT NULL,
      active INTEGER NOT NULL DEFAULT 1,
      UNIQUE(address, device_id)
    );

    CREATE TABLE IF NOT EXISTS ping_summaries (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      timestamp INTEGER NOT NULL,
      target TEXT NOT NULL,
      hop INTEGER NOT NULL,
      ip TEXT NOT NULL,
      device_id TEXT NOT NULL,
      avg_latency REAL,
      loss_pct REAL NOT NULL,
      sample_count INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_summaries_ts ON ping_summaries(timestamp);
    CREATE INDEX IF NOT EXISTS idx_summaries_target ON ping_summaries(target, hop);
    CREATE INDEX IF NOT EXISTS idx_summaries_device ON ping_summaries(device_id);

    CREATE TABLE IF NOT EXISTS ping_summaries_hourly (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      timestamp INTEGER NOT NULL,
      target TEXT NOT NULL,
      hop INTEGER NOT NULL,
      ip TEXT NOT NULL,
      device_id TEXT NOT NULL,
      avg_latency REAL,
      loss_pct REAL NOT NULL,
      sample_count INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_hourly_ts ON ping_summaries_hourly(timestamp);
    CREATE INDEX IF NOT EXISTS idx_hourly_target ON ping_summaries_hourly(target, hop);
    CREATE INDEX IF NOT EXISTS idx_hourly_device ON ping_summaries_hourly(device_id);
  `);
}
