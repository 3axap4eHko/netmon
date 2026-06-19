import type { Target, DashboardData, TimeRange } from '@netmon/shared';
import { isPresetRange } from '@netmon/shared';

const API_BASE = import.meta.env.VITE_API_URL || 'https://api.netmon.app';

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' },
    ...options,
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body || res.statusText);
  }
  return res.json();
}

export async function getDashboard(target: string, range: TimeRange): Promise<DashboardData> {
  const rangeParam = isPresetRange(range) ? range : `customDay:${range.customDay}`;
  return request(`/data/dashboard?target=${encodeURIComponent(target)}&range=${rangeParam}`);
}

export async function getTargets(): Promise<Target[]> {
  return request('/data/targets');
}

export async function login(email: string, password: string): Promise<void> {
  await request('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  });
}

export async function register(email: string, password: string): Promise<void> {
  await request('/auth/register', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  });
}

export async function logout(): Promise<void> {
  await request('/auth/logout', { method: 'POST' });
}

export async function getAccountInfo(): Promise<{ email: string; plan: string }> {
  return request('/account/info');
}

export async function createCheckoutSession(): Promise<{ url: string }> {
  return request('/account/subscribe', { method: 'POST' });
}

export async function createPortalSession(): Promise<{ url: string }> {
  return request('/account/portal', { method: 'POST' });
}
