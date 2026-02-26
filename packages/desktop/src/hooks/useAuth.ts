import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface AuthState {
  authenticated: boolean;
  userId: string | null;
  email: string | null;
  plan: string | null;
}

export function useAuth() {
  const [state, setState] = useState<AuthState>({
    authenticated: false,
    userId: null,
    email: null,
    plan: null,
  });
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    invoke<AuthState>('get_auth_state')
      .then(setState)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  const loginEmail = useCallback(async (email: string, password: string) => {
    const result = await invoke<AuthState>('login_email', { email, password });
    setState(result);
    return result;
  }, []);

  const registerEmail = useCallback(async (email: string, password: string) => {
    const result = await invoke<AuthState>('register_email', { email, password });
    setState(result);
    return result;
  }, []);

  const startOAuth = useCallback(async (provider: string) => {
    const url = await invoke<string>('start_oauth', { provider });
    // Open in default browser
    const { open } = await import('@tauri-apps/plugin-shell');
    await open(url);
  }, []);

  const logout = useCallback(async () => {
    await invoke('logout');
    setState({ authenticated: false, userId: null, email: null, plan: null });
  }, []);

  const refresh = useCallback(async () => {
    const result = await invoke<AuthState>('get_auth_state');
    setState(result);
  }, []);

  return { ...state, loading, loginEmail, registerEmail, startOAuth, logout, refresh };
}
