import { MiddlewareHandler } from 'hono';
import { verifyToken } from '../lib/jwt';

export interface AuthUser {
  sub: string;
  email: string;
  plan: string;
  maxDevices: number;
  deviceId: string;
  writeRate: number;
  retentionDays: number;
}

declare module 'hono' {
  interface ContextVariableMap {
    user: AuthUser;
  }
}

export function authMiddleware(): MiddlewareHandler {
  return async (c, next) => {
    // Try Bearer token first (desktop app)
    const authHeader = c.req.header('Authorization');
    if (authHeader?.startsWith('Bearer ')) {
      const token = authHeader.slice(7);
      try {
        const payload = await verifyToken(token, c.env.JWT_PUBLIC_KEY);
        c.set('user', payloadToUser(payload));
        return next();
      } catch {
        return c.json({ error: 'Invalid token' }, 401);
      }
    }

    // Try cookie (web dashboard)
    const cookie = c.req.header('Cookie');
    const sessionToken = parseCookie(cookie || '', 'session');
    if (sessionToken) {
      try {
        const payload = await verifyToken(sessionToken, c.env.JWT_PUBLIC_KEY);
        c.set('user', payloadToUser(payload));
        return next();
      } catch {
        return c.json({ error: 'Invalid session' }, 401);
      }
    }

    return c.json({ error: 'Authentication required' }, 401);
  };
}

function payloadToUser(payload: Record<string, unknown>): AuthUser {
  return {
    sub: payload.sub as string,
    email: payload.email as string,
    plan: (payload.plan as string) || 'free',
    maxDevices: (payload.max_devices as number) || 1,
    deviceId: (payload.device_id as string) || '',
    writeRate: (payload.write_rate as number) || 300,
    retentionDays: (payload.retention_days as number) || 1,
  };
}

function parseCookie(cookieHeader: string, name: string): string | null {
  const match = cookieHeader.match(new RegExp(`(?:^|; )${name}=([^;]*)`));
  return match ? decodeURIComponent(match[1]) : null;
}
