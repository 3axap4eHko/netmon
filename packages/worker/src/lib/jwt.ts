import * as jose from 'jose';

let cachedPrivateKey: CryptoKey | null = null;
let cachedPublicKey: CryptoKey | null = null;

async function getPrivateKey(pem: string): Promise<CryptoKey> {
  if (!cachedPrivateKey) {
    cachedPrivateKey = await jose.importPKCS8(pem, 'EdDSA');
  }
  return cachedPrivateKey;
}

async function getPublicKey(pem: string): Promise<CryptoKey> {
  if (!cachedPublicKey) {
    cachedPublicKey = await jose.importSPKI(pem, 'EdDSA');
  }
  return cachedPublicKey;
}

export interface TokenClaims {
  sub: string;
  email: string;
  plan: string;
  max_devices: number;
  device_id: string;
  write_rate: number;
  retention_days: number;
}

export async function signToken(
  claims: TokenClaims,
  privateKeyPem: string,
  expiresIn: string = '24h'
): Promise<string> {
  const key = await getPrivateKey(privateKeyPem);
  return new jose.SignJWT(claims as unknown as jose.JWTPayload)
    .setProtectedHeader({ alg: 'EdDSA' })
    .setIssuer('netmon-api')
    .setExpirationTime(expiresIn)
    .setIssuedAt()
    .sign(key);
}

export async function verifyToken(
  token: string,
  publicKeyPem: string
): Promise<Record<string, unknown>> {
  const key = await getPublicKey(publicKeyPem);
  const { payload } = await jose.jwtVerify(token, key, {
    issuer: 'netmon-api',
  });
  return payload as Record<string, unknown>;
}

export function generateRefreshToken(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

export async function hashToken(token: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(token);
  const hash = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}
