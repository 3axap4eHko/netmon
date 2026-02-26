import { useState, useEffect, useCallback } from 'react';
import type { LoadTestResult } from '@netmon/shared';
import * as api from '../api';

export type LoadTestStatus = 'idle' | 'running' | 'done';

export function useLoadTest() {
  const [status, setStatus] = useState<LoadTestStatus>('idle');
  const [result, setResult] = useState<LoadTestResult | null>(null);
  const [history, setHistory] = useState<LoadTestResult[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getLoadTestHistory().then(setHistory).catch(() => {});
  }, []);

  const runTest = useCallback(async () => {
    if (status === 'running') return;
    setStatus('running');
    setError(null);
    try {
      const r = await api.runLoadTest();
      setResult(r);
      setHistory(prev => [r, ...prev].slice(0, 20));
      setStatus('done');
    } catch (err) {
      setError(typeof err === 'string' ? err : 'Load test failed.');
      setStatus('idle');
    }
  }, [status]);

  return { status, result, history, error, runTest };
}
