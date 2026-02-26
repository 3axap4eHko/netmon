import { useState, useEffect, useCallback, useRef } from 'react';
import type { Target, DashboardData, TimeRange } from '@netmon/shared';
import * as api from '../api';

export function useDashboard() {
  const [targets, setTargets] = useState<Target[]>([]);
  const [activeTarget, setActiveTarget] = useState<string | null>(null);
  const [timeRange, setTimeRange] = useState<TimeRange>('1h');
  const [data, setData] = useState<DashboardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const inFlightRef = useRef(false);

  useEffect(() => {
    api.getTargets().then(t => {
      setTargets(t);
      if (t.length > 0) setActiveTarget(t[0].address);
      setLoading(false);
    }).catch(err => {
      console.error('Failed to load targets:', err);
      setError('Failed to load targets. Are you signed in?');
      setLoading(false);
    });
  }, []);

  const fetchData = useCallback(async () => {
    if (!activeTarget || inFlightRef.current) return;
    inFlightRef.current = true;
    try {
      const d = await api.getDashboard(activeTarget, timeRange);
      setData(d);
      setError(null);
    } catch (err) {
      console.error('Failed to fetch dashboard data:', err);
      setError('Failed to refresh dashboard data.');
    } finally {
      inFlightRef.current = false;
    }
  }, [activeTarget, timeRange]);

  useEffect(() => {
    fetchData();
    intervalRef.current = setInterval(fetchData, 30000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchData]);

  const clearError = useCallback(() => setError(null), []);

  return {
    targets,
    activeTarget,
    setActiveTarget,
    timeRange,
    setTimeRange,
    data,
    loading,
    error,
    clearError,
  };
}
