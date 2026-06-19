import { useState, useEffect, useCallback, useRef } from 'react';
import type { DashboardData, ProbeMode, Target, TimeRange } from '@netmon/shared';
import * as api from '../api';
import { loadStored, saveStored } from '../storage';

const TIME_RANGE_KEY = 'netmon.timeRange';
const ACTIVE_TARGET_KEY = 'netmon.activeTarget';

const DEFAULT_TARGETS: Array<{ address: string; label: string; probeMode: ProbeMode }> = [
  { address: '1.1.1.1', label: 'Cloudflare', probeMode: 'icmp' },
  { address: '8.8.8.8', label: 'Google DNS', probeMode: 'icmp' },
  { address: '9.9.9.9', label: 'Quad9', probeMode: 'icmp' },
  { address: '208.67.222.222', label: 'OpenDNS', probeMode: 'icmp' },
  { address: 'cf-speed-12k', label: 'Cloudflare 12KB', probeMode: 'http' },
  { address: 'cf-speed-100k', label: 'Cloudflare 100KB', probeMode: 'http' },
];

function getErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string' &&
    (error as { message: string }).message.trim()
  ) {
    return (error as { message: string }).message;
  }
  if (typeof error === 'object' && error !== null) {
    try {
      const serialized = JSON.stringify(error);
      if (serialized && serialized !== '{}' && serialized !== 'null') {
        return serialized;
      }
    } catch {
      // Ignore JSON serialization errors and fall through.
    }

    const stringified = String(error);
    if (stringified && stringified !== '[object Object]') {
      return stringified;
    }
  }
  if (typeof error === 'string' && error.trim()) {
    return error;
  }
  return fallback;
}

export function useDashboard() {
  const [targets, setTargets] = useState<Target[]>([]);
  const [activeTarget, setActiveTarget] = useState<string | null>(() => loadStored<string | null>(ACTIVE_TARGET_KEY, null));
  const [timeRange, setTimeRange] = useState<TimeRange>(() => loadStored<TimeRange>(TIME_RANGE_KEY, '1h'));
  const [data, setData] = useState<DashboardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [targetsReady, setTargetsReady] = useState(false);
  const [refreshToken, setRefreshToken] = useState(0);
  const inFlightRef = useRef(false);

  const syncTargets = useCallback((nextTargets: Target[]) => {
    setTargets(nextTargets);
    setActiveTarget(current => {
      if (nextTargets.length === 0) {
        return null;
      }
      if (current && nextTargets.some(target => target.address === current)) {
        return current;
      }
      return nextTargets[0].address;
    });
  }, []);

  const refreshTargets = useCallback(async () => {
    const nextTargets = await api.getTargets();
    syncTargets(nextTargets);
    setTargetsReady(true);
    return nextTargets;
  }, [syncTargets]);

  const triggerRefresh = useCallback(() => {
    setRefreshToken(current => current + 1);
  }, []);

  useEffect(() => {
    saveStored(TIME_RANGE_KEY, timeRange);
  }, [timeRange]);

  useEffect(() => {
    saveStored(ACTIVE_TARGET_KEY, activeTarget);
  }, [activeTarget]);

  useEffect(() => {
    let cancelled = false;

    const bootstrap = async () => {
      try {
        const nextTargets = await api.getTargets();
        if (cancelled) {
          return;
        }
        syncTargets(nextTargets);
        setError(null);
      } catch (nextError) {
        if (cancelled) {
          return;
        }
        console.error('Failed to load targets:', nextError);
        setError(getErrorMessage(nextError, 'Failed to load monitoring targets.'));
      } finally {
        if (!cancelled) {
          setTargetsReady(true);
          setLoading(false);
        }
      }
    };

    void bootstrap();

    return () => {
      cancelled = true;
    };
  }, [syncTargets]);

  useEffect(() => {
    if (!targetsReady) {
      return;
    }

    let cancelled = false;

    const syncPausedState = async () => {
      try {
        const nextPaused = await api.getMonitoringPaused();
        if (!cancelled) {
          setPaused(nextPaused);
        }
      } catch {
        // Ignore state sync failures; toggle actions still report explicit errors.
      }
    };

    void syncPausedState();
    const interval = setInterval(syncPausedState, 10000);

    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [targetsReady]);

  const fetchData = useCallback(async () => {
    if (!activeTarget || inFlightRef.current) {
      if (!activeTarget) {
        setLoading(false);
      }
      return;
    }

    inFlightRef.current = true;

    try {
      const nextData = await api.getDashboard(activeTarget, timeRange);
      setData(nextData);
      setError(null);
    } catch (nextError) {
      console.error('Failed to fetch dashboard data:', nextError);
      setError(getErrorMessage(nextError, 'Failed to refresh dashboard data.'));
    } finally {
      inFlightRef.current = false;
      setLoading(false);
    }
  }, [activeTarget, timeRange]);

  useEffect(() => {
    if (!targetsReady) {
      return;
    }

    if (!activeTarget) {
      setData(null);
      setLoading(false);
      return;
    }

    setLoading(true);
    void fetchData();
    const interval = setInterval(fetchData, 5000);

    return () => {
      clearInterval(interval);
    };
  }, [activeTarget, fetchData, refreshToken, targetsReady, timeRange]);

  useEffect(() => {
    if (!targetsReady) {
      return;
    }

    const interval = setInterval(() => {
      api.getTargets()
        .then(syncTargets)
        .catch(() => {});
    }, 30000);

    return () => {
      clearInterval(interval);
    };
  }, [syncTargets, targetsReady]);

  const handleAddTarget = useCallback(async (address: string, label: string, probeMode: ProbeMode) => {
    try {
      const target = await api.addTarget(address, label, probeMode);
      await refreshTargets();
      setActiveTarget(target.address);
      setData(null);
      setError(null);
      triggerRefresh();
    } catch (nextError) {
      console.error('Failed to add target:', nextError);
      const message = getErrorMessage(nextError, 'Failed to add target.');
      setError(message);
      throw new Error(message);
    }
  }, [refreshTargets, triggerRefresh]);

  const handleRemoveTarget = useCallback(async (id: number) => {
    try {
      await api.removeTarget(id);
      await refreshTargets();
      setData(null);
      setError(null);
      triggerRefresh();
    } catch (nextError) {
      console.error('Failed to remove target:', nextError);
      const message = getErrorMessage(nextError, 'Failed to remove target.');
      setError(message);
      throw new Error(message);
    }
  }, [refreshTargets, triggerRefresh]);

  const handleProbeModeChange = useCallback(async (address: string, probeMode: ProbeMode) => {
    try {
      await api.setProbeMode(address, probeMode);
      setTargets(current =>
        current.map(target => (
          target.address === address
            ? { ...target, probeMode }
            : target
        )),
      );
      setData(null);
      setError(null);
      triggerRefresh();
    } catch (nextError) {
      console.error('Failed to update probe mode:', nextError);
      const message = getErrorMessage(nextError, 'Failed to update probe mode.');
      setError(message);
      throw new Error(message);
    }
  }, [triggerRefresh]);

  const restoreDefaultTargets = useCallback(async () => {
    try {
      for (const target of DEFAULT_TARGETS) {
        await api.addTarget(target.address, target.label, target.probeMode);
      }
      await refreshTargets();
      setError(null);
      triggerRefresh();
    } catch (nextError) {
      console.error('Failed to restore default targets:', nextError);
      const message = getErrorMessage(nextError, 'Failed to restore the built-in targets.');
      setError(message);
      throw new Error(message);
    }
  }, [refreshTargets, triggerRefresh]);

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
    } catch (nextError) {
      console.error('Failed to toggle monitoring:', nextError);
      setError(paused ? 'Failed to resume monitoring.' : 'Failed to pause monitoring.');
    }
  }, [paused]);

  const clearError = useCallback(() => setError(null), []);
  const activeTargetDetails = targets.find(target => target.address === activeTarget) ?? null;

  return {
    targets,
    activeTarget,
    activeTargetDetails,
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
    handleProbeModeChange,
    restoreDefaultTargets,
    togglePause,
  };
}
