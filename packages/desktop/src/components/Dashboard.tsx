import React, { useState, useCallback } from 'react';
import { HopTable, LossChart, LatencyChart, TimeSelector } from '@netmon/shared';
import { useDashboard } from '../hooks/useDashboard';
import { useAuth } from '../hooks/useAuth';
import { AddTargetModal } from './AddTargetModal';
import { LoginModal } from './LoginModal';
import { AccountPage } from './AccountPage';
import { SyncIndicator } from './SyncIndicator';
import { LoadTestPanel } from './LoadTestPanel';

export const Dashboard = React.memo(function Dashboard() {
  const {
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
  } = useDashboard();

  const auth = useAuth();

  const [showAddModal, setShowAddModal] = useState(false);
  const [showLoginModal, setShowLoginModal] = useState(false);
  const [showAccountPage, setShowAccountPage] = useState(false);
  const [showLoadTest, setShowLoadTest] = useState(false);

  const onAddTarget = useCallback(
    async (address: string, label: string) => {
      await handleAddTarget(address, label);
      setShowAddModal(false);
    },
    [handleAddTarget]
  );

  const onCloseModal = useCallback(() => setShowAddModal(false), []);
  const onOpenModal = useCallback(() => setShowAddModal(true), []);

  if (loading) {
    return (
      <div className="dashboard">
        <div className="loading">
          <div className="spinner" />
          Discovering network hops...
        </div>
      </div>
    );
  }

  const activeTargets = targets.filter(t => t.active);
  const hopCount = data?.hops?.length ?? 0;

  return (
    <div className="dashboard">
      <div className="header">
        <h1>NetMon</h1>
        <div className="header-controls">
          {auth.authenticated && <SyncIndicator />}
          <TimeSelector value={timeRange} onChange={setTimeRange} />
          <button className="btn" onClick={() => setShowLoadTest(v => !v)}>
            Speed Test
          </button>
          <button className="btn" onClick={togglePause}>
            {paused ? 'Resume' : 'Pause'}
          </button>
          {auth.authenticated ? (
            <button
              className="btn"
              onClick={() => setShowAccountPage(true)}
              title={auth.email || ''}
            >
              Account
            </button>
          ) : (
            <button
              className="btn btn-primary"
              onClick={() => setShowLoginModal(true)}
            >
              Sign In
            </button>
          )}
        </div>
      </div>

      <div className="target-bar">
        {activeTargets.map(t => (
          <button
            key={t.id}
            className={`target-btn ${t.address === activeTarget ? 'active' : ''}`}
            onClick={() => setActiveTarget(t.address)}
          >
            {t.label}
            <span
              className="remove"
              onClick={(e) => { e.stopPropagation(); handleRemoveTarget(t.id); }}
            >
              x
            </span>
          </button>
        ))}
        <button className="target-btn add" onClick={onOpenModal}>
          + Add Target
        </button>
      </div>

      {error && (
        <div className="error-banner">
          <span>{error}</span>
          <button className="btn" onClick={clearError}>Dismiss</button>
        </div>
      )}

      {!data || data.hops.length === 0 ? (
        <div className="empty-state">
          <h2>Discovering hops...</h2>
          <p>The MTR engine is tracing the route to {activeTarget}. This may take a moment.</p>
        </div>
      ) : (
        <>
          <HopTable hops={data.hops} />

          <div className="charts-grid">
            <LossChart data={data.lossChart} hopCount={hopCount} />
            <LatencyChart data={data.latencyChart} hopCount={hopCount} />
          </div>
        </>
      )}

      {showLoadTest && <LoadTestPanel />}

      <div className="status-bar">
        <span>
          <span className={`status-dot ${paused ? 'yellow' : 'green'}`} />
          {paused ? 'Paused' : 'Monitoring'}
          {activeTarget ? ` — ${activeTarget}` : ''}
        </span>
        <span>
          {data?.hops?.length ?? 0} hops discovered
        </span>
      </div>

      {showAddModal && (
        <AddTargetModal onAdd={onAddTarget} onClose={onCloseModal} />
      )}

      {showLoginModal && (
        <LoginModal
          onLogin={async (e, p) => { await auth.loginEmail(e, p); }}
          onRegister={async (e, p) => { await auth.registerEmail(e, p); }}
          onOAuth={auth.startOAuth}
          onClose={() => setShowLoginModal(false)}
        />
      )}

      {showAccountPage && auth.authenticated && (
        <AccountPage
          email={auth.email || ''}
          plan={auth.plan || 'free'}
          onLogout={auth.logout}
          onClose={() => setShowAccountPage(false)}
        />
      )}
    </div>
  );
});
