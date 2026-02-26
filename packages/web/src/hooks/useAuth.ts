import { useState, useEffect, useCallback } from 'react';
import * as api from '../api';

interface AuthState {
  authenticated: boolean;
  email: string | null;
  plan: string | null;
  loading: boolean;
}

export function useAuth() {
  const [state, setState] = useState<AuthState>({
    authenticated: false,
    email: null,
    plan: null,
    loading: true,
  });

  useEffect(() => {
    api.getAccountInfo()
      .then(info => {
        setState({
          authenticated: true,
          email: info.email,
          plan: info.plan,
          loading: false,
        });
      })
      .catch(() => {
        setState(prev => ({ ...prev, loading: false }));
      });
  }, []);

  const login = useCallback(async (email: string, password: string) => {
    await api.login(email, password);
    const info = await api.getAccountInfo();
    setState({
      authenticated: true,
      email: info.email,
      plan: info.plan,
      loading: false,
    });
  }, []);

  const register = useCallback(async (email: string, password: string) => {
    await api.register(email, password);
    const info = await api.getAccountInfo();
    setState({
      authenticated: true,
      email: info.email,
      plan: info.plan,
      loading: false,
    });
  }, []);

  const logout = useCallback(async () => {
    await api.logout();
    setState({ authenticated: false, email: null, plan: null, loading: false });
  }, []);

  return { ...state, login, register, logout };
}
