// Apple Sign In OAuth implementation
// To be configured with Apple Developer credentials

const APPLE_AUTH_URL = 'https://appleid.apple.com/auth/authorize';
const APPLE_TOKEN_URL = 'https://appleid.apple.com/auth/token';

export function startAppleOAuth(clientId: string): { url: string; state: string } {
  const state = crypto.randomUUID();
  const params = new URLSearchParams({
    client_id: clientId,
    redirect_uri: 'https://api.netmon.app/auth/apple/callback',
    response_type: 'code',
    scope: 'name email',
    response_mode: 'form_post',
    state,
  });

  return { url: `${APPLE_AUTH_URL}?${params}`, state };
}

export async function handleAppleCallback(
  code: string,
  clientId: string,
  clientSecret: string
): Promise<{ email: string; providerId: string }> {
  const tokenRes = await fetch(APPLE_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      code,
      client_id: clientId,
      client_secret: clientSecret,
      redirect_uri: 'https://api.netmon.app/auth/apple/callback',
      grant_type: 'authorization_code',
    }),
  });

  if (!tokenRes.ok) {
    throw new Error(`Apple token exchange failed: ${tokenRes.status}`);
  }

  const tokens = await tokenRes.json() as { id_token: string };

  // Decode the ID token (Apple uses JWT)
  const parts = tokens.id_token.split('.');
  const payload = JSON.parse(atob(parts[1]));

  return {
    email: payload.email,
    providerId: payload.sub,
  };
}
