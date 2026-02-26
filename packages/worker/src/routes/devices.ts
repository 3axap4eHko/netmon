import { Hono } from 'hono';
import type { Env } from '../index';
import { getSystemDb } from '../lib/turso';

const devices = new Hono<{ Bindings: Env }>();

devices.get('/', async (c) => {
  const user = c.get('user');
  const db = getSystemDb(c.env);

  const result = await db.execute({
    sql: 'SELECT id, name, platform, last_push_at, created_at FROM devices WHERE user_id = ? ORDER BY last_push_at DESC',
    args: [user.sub],
  });

  return c.json(result.rows.map((r) => ({
    id: r.id as string,
    name: r.name as string,
    platform: r.platform as string,
    lastPushAt: r.last_push_at as number | null,
    createdAt: r.created_at as number,
  })));
});

devices.delete('/:id', async (c) => {
  const user = c.get('user');
  const deviceId = c.req.param('id');
  const db = getSystemDb(c.env);

  await db.execute({
    sql: 'DELETE FROM devices WHERE id = ? AND user_id = ?',
    args: [deviceId, user.sub],
  });

  // Also clean up refresh tokens for this device
  await db.execute({
    sql: 'DELETE FROM refresh_tokens WHERE device_id = ? AND user_id = ?',
    args: [deviceId, user.sub],
  });

  return c.json({ ok: true });
});

export { devices as deviceRoutes };
