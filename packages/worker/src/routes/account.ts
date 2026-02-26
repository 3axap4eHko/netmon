import { Hono } from 'hono';
import type { Env } from '../index';
import { getSystemDb } from '../lib/turso';
import { getStripe, createCheckoutSession, createPortalSession, getOrCreateCustomer } from '../lib/stripe';

const account = new Hono<{ Bindings: Env }>();

account.get('/info', async (c) => {
  const user = c.get('user');
  const db = getSystemDb(c.env);

  const result = await db.execute({
    sql: 'SELECT email, plan FROM users WHERE id = ?',
    args: [user.sub],
  });

  if (result.rows.length === 0) {
    return c.json({ error: 'User not found' }, 404);
  }

  return c.json({
    email: result.rows[0].email as string,
    plan: (result.rows[0].plan as string) || 'free',
  });
});

account.post('/subscribe', async (c) => {
  const user = c.get('user');
  const db = getSystemDb(c.env);
  const stripe = getStripe(c.env.STRIPE_SECRET_KEY);

  const userResult = await db.execute({
    sql: 'SELECT email, stripe_customer_id FROM users WHERE id = ?',
    args: [user.sub],
  });

  const row = userResult.rows[0];
  const customerId = await getOrCreateCustomer(
    stripe, row.email as string, row.stripe_customer_id as string | null
  );

  // Save customer ID if new
  if (!row.stripe_customer_id) {
    await db.execute({
      sql: 'UPDATE users SET stripe_customer_id = ? WHERE id = ?',
      args: [customerId, user.sub],
    });
  }

  const url = await createCheckoutSession(
    stripe,
    customerId,
    'https://netmon.app/account?success=true',
    'https://netmon.app/account?canceled=true'
  );

  return c.json({ url });
});

account.post('/portal', async (c) => {
  const user = c.get('user');
  const db = getSystemDb(c.env);
  const stripe = getStripe(c.env.STRIPE_SECRET_KEY);

  const userResult = await db.execute({
    sql: 'SELECT stripe_customer_id FROM users WHERE id = ?',
    args: [user.sub],
  });

  const customerId = userResult.rows[0]?.stripe_customer_id as string | null;
  if (!customerId) {
    return c.json({ error: 'No billing account' }, 400);
  }

  const url = await createPortalSession(stripe, customerId, 'https://netmon.app/account');
  return c.json({ url });
});

export { account as accountRoutes };
