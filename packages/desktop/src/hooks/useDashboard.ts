import { useState, useEffect, useCallback, useRef } from 'react';
import type { Target, DashboardData, TimeRange } from '@netmon/shared';
import * as api from '../api';

export function useDashboard() {
  const [targets, setTargets] = useState<Target[]>([]);
  const [activeTarget, setActiveTarget] = useState<string | null>(null);
  const [timeRange, setTimeRange] = useState<TimeRange>('1h');
  const [data, setData] = useState<DashboardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const inFlightRef = useRef(false);

  // Load targets on mount
  useEffect(() => {
    api.getTargets().then(t => {
      setTargets(t);
      const active = t.find(x => x.active) ?? t[0];
      if (active) setActiveTarget(active.address);
      setLoading(false);
    }).catch(err => {
      console.error('Failed to load targets:', err);
      setError('Failed to load targets.');
      setLoading(false);
    });
  }, []);

  // Fetch dashboard data
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

  // Auto-refresh every 5 seconds
  useEffect(() => {
    fetchData();
    intervalRef.current = setInterval(fetchData, 5000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchData]);

  const handleAddTarget = useCallback(async (address: string, label: string) => {
    try {
      const target = await api.addTarget(address, label);
      setTargets(prev => {
        const exists = prev.find(t => t.address === target.address);
        if (exists) return prev.map(t => t.address === target.address ? target : t);
        return [...prev, target];
      });
      setActiveTarget(target.address);
      setError(null);
    } catch (err) {
      console.error('Failed to add target:', err);
      setError(typeof err === 'string' ? err : 'Failed to add target.');
      throw err;
    }
  }, []);

  const handleRemoveTarget = useCallback(async (id: number) => {
    try {
      await api.removeTarget(id);
      setTargets(prev => {
        const removed = prev.find(t => t.id === id);
        const remaining = prev.filter(t => t.id !== id);
        if (remaining.length === 0) {
          setActiveTarget(null);
        } else {
          const activeAddressStillExists = activeTarget
            ? remaining.some(t => t.address === activeTarget)
            : false;
          if (!activeAddressStillExists || removed?.address === activeTarget) {
            const nextActive = remaining.find(t => t.active) ?? remaining[0];
            setActiveTarget(nextActive.address);
          }
        }
        return remaining;
      });
      setError(null);
    } catch (err) {
      console.error('Failed to remove target:', err);
      setError('Failed to remove target.');
      throw err;
    }
  }, [activeTarget]);

  const togglePause = useCallback(async () => {
    try {
      if (paused) {
        await api.resumeMonitoring();
        setPaused(false);
      } else {
        await api.pauseMonitoring();
        setPaused(true);
      }
      setError(null);
    } catch (err) {
      console.error('Failed to toggle monitoring:', err);
      setError(paused ? 'Failed to resume monitoring.' : 'Failed to pause monitoring.');
      throw err;
    }
  }, [paused]);

  const clearError = useCallback(() => setError(null), []);

  return {
    targets,
    activeTarget,
    setActiveTarget,
    timeRange,
    setTimeRange,
    data,
    loading,
    paused,
    error,
    clearError,
    handleAddTarget,
    handleRemoveTarget,
    togglePause,
  };
}
