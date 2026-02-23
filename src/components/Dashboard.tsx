import React, { useState, useCallback } from 'react';
import { useDashboard } from '../hooks/useDashboard';
import { HopTable } from './HopTable';
import { LossChart } from './LossChart';
import { LatencyChart } from './LatencyChart';
import { TimeSelector } from './TimeSelector';
import { AddTargetModal } from './AddTargetModal';

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

  const [showAddModal, setShowAddModal] = useState(false);

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
          <TimeSelector value={timeRange} onChange={setTimeRange} />
          <button className="btn" onClick={togglePause}>
            {paused ? 'Resume' : 'Pause'}
          </button>
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
    </div>
  );
});
