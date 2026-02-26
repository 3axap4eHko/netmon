const GOOGLE_AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth';
const GOOGLE_TOKEN_URL = 'https://oauth2.googleapis.com/token';
const GOOGLE_USERINFO_URL = 'https://www.googleapis.com/oauth2/v2/userinfo';

export function startGoogleOAuth(clientId: string): { url: string; state: string } {
  const state = crypto.randomUUID();
  const params = new URLSearchParams({
    client_id: clientId,
    redirect_uri: 'https://api.netmon.app/auth/google/callback',
    response_type: 'code',
    scope: 'openid email profile',
    state,
    access_type: 'offline',
    prompt: 'consent',
  });

  return { url: `${GOOGLE_AUTH_URL}?${params}`, state };
}

export async function handleGoogleCallback(
  code: string,
  clientId: string,
  clientSecret: string
): Promise<{ email: string; providerId: string; name?: string }> {
  // Exchange code for tokens
  const tokenRes = await fetch(GOOGLE_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      code,
      client_id: clientId,
      client_secret: clientSecret,
      redirect_uri: 'https://api.netmon.app/auth/google/callback',
      grant_type: 'authorization_code',
    }),
  });

  if (!tokenRes.ok) {
    throw new Error(`Google token exchange failed: ${tokenRes.status}`);
  }

  const tokens = await tokenRes.json() as { access_token: string };

  // Get user info
  const userRes = await fetch(GOOGLE_USERINFO_URL, {
    headers: { Authorization: `Bearer ${tokens.access_token}` },
  });

  if (!userRes.ok) {
    throw new Error(`Google userinfo failed: ${userRes.status}`);
  }

  const userInfo = await userRes.json() as { id: string; email: string; name?: string };

  return {
    email: userInfo.email,
    providerId: userInfo.id,
    name: userInfo.name,
  };
}
