import { Hono } from 'hono';
import type { Env } from '../index';
import { getSystemDb } from '../lib/turso';
import { getStripe } from '../lib/stripe';

const webhooks = new Hono<{ Bindings: Env }>();

webhooks.post('/stripe', async (c) => {
  const signature = c.req.header('stripe-signature');
  if (!signature) {
    return c.json({ error: 'Missing signature' }, 400);
  }

  const rawBody = await c.req.text();
  const stripe = getStripe(c.env.STRIPE_SECRET_KEY);

  let event;
  try {
    event = await stripe.webhooks.constructEventAsync(
      rawBody,
      signature,
      c.env.STRIPE_WEBHOOK_SECRET
    );
  } catch {
    return c.json({ error: 'Invalid signature' }, 400);
  }

  const db = getSystemDb(c.env);

  switch (event.type) {
    case 'checkout.session.completed': {
      const session = event.data.object as any;
      const customerId = session.customer as string;
      const subscriptionId = session.subscription as string;

      // Find user by customer ID
      const userResult = await db.execute({
        sql: 'SELECT id FROM users WHERE stripe_customer_id = ?',
        args: [customerId],
      });

      if (userResult.rows.length > 0) {
        const userId = userResult.rows[0].id as string;
        await db.execute({
          sql: 'UPDATE users SET plan = ? WHERE id = ?',
          args: ['pro', userId],
        });
        await db.execute({
          sql: `INSERT INTO subscriptions (user_id, stripe_subscription_id, status, current_period_end)
                VALUES (?, ?, 'active', ?)
                ON CONFLICT(stripe_subscription_id) DO UPDATE SET status = 'active'`,
          args: [userId, subscriptionId, Math.floor(Date.now() / 1000) + 30 * 86400],
        });
      }
      break;
    }

    case 'invoice.paid': {
      const invoice = event.data.object as any;
      const customerId = invoice.customer as string;
      const subscriptionId = invoice.subscription as string;

      const userResult = await db.execute({
        sql: 'SELECT id FROM users WHERE stripe_customer_id = ?',
        args: [customerId],
      });

      if (userResult.rows.length > 0) {
        const userId = userResult.rows[0].id as string;
        await db.execute({
          sql: 'UPDATE users SET plan = ? WHERE id = ?',
          args: ['pro', userId],
        });
        await db.execute({
          sql: 'UPDATE subscriptions SET status = ?, current_period_end = ? WHERE stripe_subscription_id = ?',
          args: ['active', Math.floor(Date.now() / 1000) + 30 * 86400, subscriptionId],
        });
      }
      break;
    }

    case 'customer.subscription.updated': {
      const subscription = event.data.object as any;
      const customerId = subscription.customer as string;
      const status = subscription.status as string;

      const userResult = await db.execute({
        sql: 'SELECT id FROM users WHERE stripe_customer_id = ?',
        args: [customerId],
      });

      if (userResult.rows.length > 0) {
        const userId = userResult.rows[0].id as string;
        const plan = status === 'active' ? 'pro' : 'free';
        await db.execute({
          sql: 'UPDATE users SET plan = ? WHERE id = ?',
          args: [plan, userId],
        });
        await db.execute({
          sql: 'UPDATE subscriptions SET status = ?, current_period_end = ? WHERE stripe_subscription_id = ?',
          args: [status, subscription.current_period_end, subscription.id],
        });
      }
      break;
    }

    case 'customer.subscription.deleted': {
      const subscription = event.data.object as any;
      const customerId = subscription.customer as string;

      const userResult = await db.execute({
        sql: 'SELECT id FROM users WHERE stripe_customer_id = ?',
        args: [customerId],
      });

      if (userResult.rows.length > 0) {
        const userId = userResult.rows[0].id as string;
        await db.execute({
          sql: 'UPDATE users SET plan = ? WHERE id = ?',
          args: ['free', userId],
        });
        await db.execute({
          sql: 'UPDATE subscriptions SET status = ? WHERE stripe_subscription_id = ?',
          args: ['canceled', subscription.id],
        });
      }
      break;
    }
  }

  return c.json({ received: true });
});

export { webhooks as webhookRoutes };
