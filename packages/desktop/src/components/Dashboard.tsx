import React, { useEffect, useRef, useState } from 'react';
import { HopTable, LossChart, LatencyChart, TimeSelector } from '@netmon/shared';
import type { ProbeMode } from '@netmon/shared';
import { useDashboard } from '../hooks/useDashboard';
import { loadStored, saveStored } from '../storage';
import { useAuth } from '../hooks/useAuth';
import { LoginModal } from './LoginModal';
import { AccountPage } from './AccountPage';
import { SyncIndicator } from './SyncIndicator';
import { LoadTestPanel } from './LoadTestPanel';
import { ReportPanel } from './ReportPanel';

const PROBE_LABELS: Record<ProbeMode, string> = {
  icmp: 'ICMP 32B',
  'icmp-large': 'ICMP 1472B',
  http: 'HTTP Upload',
};

const SHOW_LOAD_TEST_KEY = 'netmon.showLoadTest';
const CHART_VIEWPORT_KEY = 'netmon.chartViewport';

export const Dashboard = React.memo(function Dashboard() {
  const {
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
    handleProbeModeChange,
    restoreDefaultTargets,
    togglePause,
  } = useDashboard();

  const auth = useAuth();

  const [showLoginModal, setShowLoginModal] = useState(false);
  const [showAccountPage, setShowAccountPage] = useState(false);
  const [showReport, setShowReport] = useState(false);
  const [showLoadTest, setShowLoadTest] = useState(() => loadStored(SHOW_LOAD_TEST_KEY, false));
  const [chartViewport, setChartViewport] = useState<[number, number] | null>(() => loadStored<[number, number] | null>(CHART_VIEWPORT_KEY, null));

  const skipViewportReset = useRef(true);
  useEffect(() => {
    if (skipViewportReset.current) {
      skipViewportReset.current = false;
      return;
    }
    setChartViewport(null);
  }, [activeTarget, timeRange]);

  useEffect(() => {
    saveStored(CHART_VIEWPORT_KEY, chartViewport);
  }, [chartViewport]);

  useEffect(() => {
    saveStored(SHOW_LOAD_TEST_KEY, showLoadTest);
  }, [showLoadTest]);

  const hopCount = data?.hops?.length ?? 0;
  const probeLabel = activeTargetDetails ? PROBE_LABELS[activeTargetDetails.probeMode] : 'Idle';
  const isHttpTarget = activeTargetDetails?.probeMode === 'http';

  if (loading && !data && targets.length === 0) {
    return (
      <div className="dashboard">
        <div className="loading loading-panel panel-card">
          <div className="spinner" />
          Loading dashboard...
        </div>
      </div>
    );
  }

  return (
    <div className="dashboard">
      <header className="panel-card monitor-bar">
        {targets.length > 0 && (
          <div className="monitor-controls">
            <label className="toolbar-field toolbar-target">
              <span className="toolbar-label">Target</span>
              <select
                value={activeTarget ?? ''}
                onChange={event => setActiveTarget(event.target.value)}
              >
                {targets.map(target => (
                  <option key={target.id} value={target.address}>
                    {target.label} · {target.address}
                  </option>
                ))}
              </select>
            </label>

            <div className="toolbar-field toolbar-profile">
              <span className="toolbar-label">Profile</span>
              {activeTargetDetails && !isHttpTarget ? (
                <select
                  value={activeTargetDetails.probeMode}
                  onChange={event => void handleProbeModeChange(
                    activeTargetDetails.address,
                    event.target.value as ProbeMode,
                  )}
                >
                  <option value="icmp">ICMP 32B</option>
                  <option value="icmp-large">ICMP 1472B</option>
                </select>
              ) : (
                <div className="field-readout">{probeLabel}</div>
              )}
            </div>

            <div className="toolbar-field toolbar-range">
              <span className="toolbar-label">Range</span>
              <TimeSelector value={timeRange} onChange={setTimeRange} />
            </div>

            <div className="toolbar-meta">
              {auth.authenticated && <SyncIndicator />}
              <div className={`status-pill ${paused ? 'paused' : ''}`}>
                <span className={`status-dot ${paused ? 'yellow' : 'green'}`} />
                {paused ? 'Paused' : 'Live'}
              </div>
            </div>

            <div className="monitor-actions">
              <button className="btn" onClick={() => setShowReport(true)}>
                Report
              </button>
              <button className="btn" onClick={() => setShowLoadTest(value => !value)}>
                {showLoadTest ? 'Hide Speed Test' : 'Speed Test'}
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
                <button className="btn btn-primary" onClick={() => setShowLoginModal(true)}>
                  Sign In
                </button>
              )}
            </div>
          </div>
        )}
      </header>

      {error && (
        <div className="error-banner">
          <span>{error}</span>
          <button className="btn" onClick={clearError}>Dismiss</button>
        </div>
      )}

      {targets.length === 0 ? (
        <div className="empty-state panel-card">
          <h2>No monitoring targets loaded</h2>
          <p>Restore the built-in targets first, then refine the route dashboard from there.</p>
          <div className="empty-action-row">
            <button className="btn btn-primary" onClick={() => void restoreDefaultTargets()}>
              Restore Built-In Targets
            </button>
          </div>
        </div>
      ) : !activeTargetDetails ? (
        <div className="empty-state panel-card">
          <h2>No target selected</h2>
          <p>Pick a target to inspect its current route and timeline.</p>
        </div>
      ) : (
        <>
          {loading ? (
            <div className="loading loading-panel panel-card">
              <div className="spinner" />
              Refreshing route data...
            </div>
          ) : !data || data.hops.length === 0 ? (
            <div className="empty-state panel-card">
              <h2>{isHttpTarget ? 'Starting HTTP probes...' : 'Discovering hops...'}</h2>
              <p>
                {isHttpTarget
                  ? `Sending HTTP upload probes for ${activeTargetDetails.label}.`
                  : `Tracing the route to ${activeTargetDetails.label} (${activeTargetDetails.address}).`}
              </p>
            </div>
          ) : (
            <div className="workspace">
              <section className="route-section">
                <HopTable hops={data.hops} />
              </section>

              <section className="charts-row">
                <LossChart
                  data={data.lossChart}
                  hopCount={hopCount}
                  timeRange={timeRange}
                  height={220}
                  viewport={chartViewport}
                  onViewportChange={setChartViewport}
                />
                <LatencyChart
                  data={data.latencyChart}
                  hopCount={hopCount}
                  timeRange={timeRange}
                  height={220}
                  viewport={chartViewport}
                  onViewportChange={setChartViewport}
                />
              </section>
            </div>
          )}

          {showLoadTest && <LoadTestPanel />}
        </>
      )}

      {showLoginModal && (
        <LoginModal
          onLogin={async (email, password) => { await auth.loginEmail(email, password); }}
          onRegister={async (email, password) => { await auth.registerEmail(email, password); }}
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

      {showReport && <ReportPanel onClose={() => setShowReport(false)} />}
    </div>
  );
});
