import React from 'react';
import { HopTable, LossChart, LatencyChart, TimeSelector } from '@netmon/shared';
import { useDashboard } from '../hooks/useDashboard';
import { useAuth } from '../hooks/useAuth';
import { LoginPage } from './LoginPage';

export function Dashboard() {
  const auth = useAuth();
  const {
    targets,
    activeTarget,
    setActiveTarget,
    timeRange,
    setTimeRange,
    data,
    loading,
    error,
    clearError,
  } = useDashboard();

  if (auth.loading) {
    return (
      <div className="app">
        <div className="loading">
          <div className="spinner" />
          Loading...
        </div>
      </div>
    );
  }

  if (!auth.authenticated) {
    return <LoginPage onLogin={auth.login} onRegister={auth.register} />;
  }

  const hopCount = data?.hops?.length ?? 0;

  return (
    <div className="app">
      <div className="header">
        <h1>NetMon</h1>
        <div className="header-controls">
          <TimeSelector value={timeRange} onChange={setTimeRange} />
          <span style={{ fontSize: 12, color: '#8b949e' }}>{auth.email}</span>
          <button className="btn" onClick={auth.logout}>Sign Out</button>
        </div>
      </div>

      {targets.length > 0 && (
        <div className="target-bar">
          {targets.map(t => (
            <button
              key={t.id}
              className={`target-btn ${t.address === activeTarget ? 'active' : ''}`}
              onClick={() => setActiveTarget(t.address)}
            >
              {t.label}
            </button>
          ))}
        </div>
      )}

      {error && (
        <div className="error-banner">
          <span>{error}</span>
          <button className="btn" onClick={clearError}>Dismiss</button>
        </div>
      )}

      {loading ? (
        <div className="loading">
          <div className="spinner" />
          Loading dashboard...
        </div>
      ) : !data || data.hops.length === 0 ? (
        <div className="empty-state">
          <h2>No data yet</h2>
          <p>Make sure your desktop app is running and syncing.</p>
        </div>
      ) : (
        <>
          <HopTable hops={data.hops} />
          <div className="charts-grid">
            <LossChart data={data.lossChart} hopCount={hopCount} timeRange={timeRange} />
            <LatencyChart data={data.latencyChart} hopCount={hopCount} timeRange={timeRange} />
          </div>
        </>
      )}
    </div>
  );
}
