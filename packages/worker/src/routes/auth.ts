import { Hono } from 'hono';
import type { Env } from '../index';
import { getSystemDb } from '../lib/turso';
import { signToken, generateRefreshToken, hashToken } from '../lib/jwt';
import { startGoogleOAuth, handleGoogleCallback } from '../lib/oauth/google';

const auth = new Hono<{ Bindings: Env }>();

// Email/password registration
auth.post('/register', async (c) => {
  const { email, password } = await c.req.json<{ email: string; password: string }>();

  if (!email || !password || password.length < 8) {
    return c.json({ error: 'Email and password (8+ chars) required' }, 400);
  }

  const db = getSystemDb(c.env);
  const userId = `usr_${crypto.randomUUID().replace(/-/g, '').slice(0, 12)}`;

  // Hash password with Web Crypto
  const encoder = new TextEncoder();
  const salt = crypto.randomUUID();
  const keyMaterial = await crypto.subtle.importKey(
    'raw', encoder.encode(password), 'PBKDF2', false, ['deriveBits']
  );
  const derived = await crypto.subtle.deriveBits(
    { name: 'PBKDF2', salt: encoder.encode(salt), iterations: 100000, hash: 'SHA-256' },
    keyMaterial, 256
  );
  const passwordHash = `${salt}:${Array.from(new Uint8Array(derived)).map(b => b.toString(16).padStart(2, '0')).join('')}`;

  try {
    await db.execute({
      sql: 'INSERT INTO users (id, email, password_hash) VALUES (?, ?, ?)',
      args: [userId, email.toLowerCase(), passwordHash],
    });
  } catch (e: any) {
    if (e.message?.includes('UNIQUE')) {
      return c.json({ error: 'Email already registered' }, 409);
    }
    throw e;
  }

  const claims = buildClaims(userId, email, 'free', '', 1, 300, 1);
  const accessToken = await signToken(claims, c.env.JWT_PRIVATE_KEY);
  const refreshToken = generateRefreshToken();

  await db.execute({
    sql: 'INSERT INTO refresh_tokens (user_id, device_id, token_hash, expires_at) VALUES (?, ?, ?, ?)',
    args: [userId, '', await hashToken(refreshToken), Math.floor(Date.now() / 1000) + 30 * 86400],
  });

  setSessionCookie(c, accessToken);
  return c.json({ access_token: accessToken, refresh_token: refreshToken, user_id: userId });
});

// Email/password login
auth.post('/login', async (c) => {
  const { email, password } = await c.req.json<{ email: string; password: string }>();

  const db = getSystemDb(c.env);
  const result = await db.execute({
    sql: 'SELECT id, email, password_hash, plan FROM users WHERE email = ?',
    args: [email.toLowerCase()],
  });

  if (result.rows.length === 0) {
    return c.json({ error: 'Invalid credentials' }, 401);
  }

  const user = result.rows[0];
  const storedHash = user.password_hash as string;
  if (!storedHash) {
    return c.json({ error: 'Use OAuth to sign in' }, 400);
  }

  const [salt, hash] = storedHash.split(':');
  const encoder = new TextEncoder();
  const keyMaterial = await crypto.subtle.importKey(
    'raw', encoder.encode(password), 'PBKDF2', false, ['deriveBits']
  );
  const derived = await crypto.subtle.deriveBits(
    { name: 'PBKDF2', salt: encoder.encode(salt), iterations: 100000, hash: 'SHA-256' },
    keyMaterial, 256
  );
  const computedHash = Array.from(new Uint8Array(derived)).map(b => b.toString(16).padStart(2, '0')).join('');

  if (computedHash !== hash) {
    return c.json({ error: 'Invalid credentials' }, 401);
  }

  const plan = (user.plan as string) || 'free';
  const retentionDays = plan === 'pro' ? 30 : 1;
  const claims = buildClaims(user.id as string, user.email as string, plan, '', 1, 300, retentionDays);
  const accessToken = await signToken(claims, c.env.JWT_PRIVATE_KEY);
  const refreshToken = generateRefreshToken();

  await db.execute({
    sql: 'INSERT INTO refresh_tokens (user_id, device_id, token_hash, expires_at) VALUES (?, ?, ?, ?)',
    args: [user.id as string, '', await hashToken(refreshToken), Math.floor(Date.now() / 1000) + 30 * 86400],
  });

  setSessionCookie(c, accessToken);
  return c.json({ access_token: accessToken, refresh_token: refreshToken });
});

// Exchange auth code + PKCE verifier for tokens (used by desktop OAuth flow)
auth.post('/token', async (c) => {
  const { code, code_verifier, device_id } = await c.req.json<{
    code: string;
    code_verifier: string;
    device_id: string;
  }>();

  const db = getSystemDb(c.env);
  const result = await db.execute({
    sql: 'SELECT user_id, code_challenge, device_id, expires_at FROM auth_codes WHERE code = ?',
    args: [code],
  });

  if (result.rows.length === 0) {
    return c.json({ error: 'Invalid auth code' }, 400);
  }

  const row = result.rows[0];

  // Verify PKCE
  const encoder = new TextEncoder();
  const challengeBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(code_verifier));
  const challenge = btoa(String.fromCharCode(...new Uint8Array(challengeBuffer)))
    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

  if (challenge !== row.code_challenge) {
    return c.json({ error: 'Invalid code verifier' }, 400);
  }

  if (Date.now() / 1000 > (row.expires_at as number)) {
    return c.json({ error: 'Auth code expired' }, 400);
  }

  // Delete used code
  await db.execute({ sql: 'DELETE FROM auth_codes WHERE code = ?', args: [code] });

  // Get user
  const userResult = await db.execute({
    sql: 'SELECT id, email, plan FROM users WHERE id = ?',
    args: [row.user_id as string],
  });
  const user = userResult.rows[0];
  const plan = (user.plan as string) || 'free';
  const retentionDays = plan === 'pro' ? 30 : 1;
  const claims = buildClaims(user.id as string, user.email as string, plan, device_id, 1, 300, retentionDays);
  const accessToken = await signToken(claims, c.env.JWT_PRIVATE_KEY);
  const refreshToken = generateRefreshToken();

  await db.execute({
    sql: 'INSERT INTO refresh_tokens (user_id, device_id, token_hash, expires_at) VALUES (?, ?, ?, ?)',
    args: [user.id as string, device_id, await hashToken(refreshToken), Math.floor(Date.now() / 1000) + 30 * 86400],
  });

  return c.json({ access_token: accessToken, refresh_token: refreshToken });
});

// Refresh access token
auth.post('/refresh', async (c) => {
  const { refresh_token, device_id } = await c.req.json<{ refresh_token: string; device_id?: string }>();

  const db = getSystemDb(c.env);
  const tokenHash = await hashToken(refresh_token);
  const result = await db.execute({
    sql: 'SELECT user_id, device_id, expires_at FROM refresh_tokens WHERE token_hash = ?',
    args: [tokenHash],
  });

  if (result.rows.length === 0) {
    return c.json({ error: 'Invalid refresh token' }, 401);
  }

  const row = result.rows[0];
  if (Date.now() / 1000 > (row.expires_at as number)) {
    await db.execute({ sql: 'DELETE FROM refresh_tokens WHERE token_hash = ?', args: [tokenHash] });
    return c.json({ error: 'Refresh token expired' }, 401);
  }

  const userResult = await db.execute({
    sql: 'SELECT id, email, plan FROM users WHERE id = ?',
    args: [row.user_id as string],
  });
  const user = userResult.rows[0];
  const plan = (user.plan as string) || 'free';
  const retentionDays = plan === 'pro' ? 30 : 1;
  const devId = device_id || (row.device_id as string) || '';
  const claims = buildClaims(user.id as string, user.email as string, plan, devId, 1, 300, retentionDays);
  const accessToken = await signToken(claims, c.env.JWT_PRIVATE_KEY);

  setSessionCookie(c, accessToken);
  return c.json({ access_token: accessToken });
});

// Google OAuth initiation
auth.get('/google', async (c) => {
  const { url, state } = startGoogleOAuth(c.env.GOOGLE_CLIENT_ID);
  return c.redirect(url);
});

// Google OAuth callback
auth.get('/google/callback', async (c) => {
  const code = c.req.query('code');
  if (!code) return c.json({ error: 'Missing code' }, 400);

  const { email, providerId } = await handleGoogleCallback(
    code, c.env.GOOGLE_CLIENT_ID, c.env.GOOGLE_CLIENT_SECRET
  );

  const db = getSystemDb(c.env);

  // Check if OAuth identity exists
  let userResult = await db.execute({
    sql: 'SELECT u.id, u.email, u.plan FROM users u JOIN oauth_identities o ON u.id = o.user_id WHERE o.provider = ? AND o.provider_user_id = ?',
    args: ['google', providerId],
  });

  let userId: string;
  let plan: string;

  if (userResult.rows.length > 0) {
    userId = userResult.rows[0].id as string;
    plan = (userResult.rows[0].plan as string) || 'free';
  } else {
    // Check if user exists by email
    const existingUser = await db.execute({
      sql: 'SELECT id, plan FROM users WHERE email = ?',
      args: [email.toLowerCase()],
    });

    if (existingUser.rows.length > 0) {
      userId = existingUser.rows[0].id as string;
      plan = (existingUser.rows[0].plan as string) || 'free';
    } else {
      userId = `usr_${crypto.randomUUID().replace(/-/g, '').slice(0, 12)}`;
      plan = 'free';
      await db.execute({
        sql: 'INSERT INTO users (id, email) VALUES (?, ?)',
        args: [userId, email.toLowerCase()],
      });
    }

    await db.execute({
      sql: 'INSERT INTO oauth_identities (user_id, provider, provider_user_id) VALUES (?, ?, ?)',
      args: [userId, 'google', providerId],
    });
  }

  // Generate auth code for PKCE exchange
  const authCode = crypto.randomUUID();
  const codeChallenge = c.req.query('code_challenge') || '';
  const deviceId = c.req.query('device_id') || '';

  await db.execute({
    sql: 'INSERT INTO auth_codes (code, user_id, code_challenge, device_id, expires_at) VALUES (?, ?, ?, ?, ?)',
    args: [authCode, userId, codeChallenge, deviceId, Math.floor(Date.now() / 1000) + 600],
  });

  // Redirect to desktop app via deep link
  const redirectUrl = `netmon://auth/callback?code=${authCode}`;
  return c.redirect(redirectUrl);
});

// Logout (clear cookie)
auth.post('/logout', async (c) => {
  c.res.headers.set('Set-Cookie', 'session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0');
  return c.json({ ok: true });
});

function buildClaims(
  userId: string, email: string, plan: string,
  deviceId: string, maxDevices: number, writeRate: number, retentionDays: number
) {
  return {
    sub: userId,
    email,
    plan,
    max_devices: maxDevices,
    device_id: deviceId,
    write_rate: writeRate,
    retention_days: retentionDays,
  };
}

function setSessionCookie(c: any, token: string) {
  c.res.headers.append(
    'Set-Cookie',
    `session=${token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=${24 * 3600}`
  );
}

export { auth as authRoutes };
