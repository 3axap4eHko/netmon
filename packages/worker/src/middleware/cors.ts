import { MiddlewareHandler } from 'hono';

const ALLOWED_ORIGINS = [
  'https://netmon.app',
  'https://www.netmon.app',
  'http://localhost:1420',
  'http://localhost:1421',
  'tauri://localhost',
];

export function cors(): MiddlewareHandler {
  return async (c, next) => {
    const origin = c.req.header('Origin') || '';
    const isAllowed = ALLOWED_ORIGINS.includes(origin);

    if (c.req.method === 'OPTIONS') {
      return new Response(null, {
        status: 204,
        headers: {
          'Access-Control-Allow-Origin': isAllowed ? origin : '',
          'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
          'Access-Control-Allow-Headers': 'Content-Type, Authorization',
          'Access-Control-Allow-Credentials': 'true',
          'Access-Control-Max-Age': '86400',
        },
      });
    }

    await next();

    if (isAllowed) {
      c.res.headers.set('Access-Control-Allow-Origin', origin);
      c.res.headers.set('Access-Control-Allow-Credentials', 'true');
    }
  };
}
