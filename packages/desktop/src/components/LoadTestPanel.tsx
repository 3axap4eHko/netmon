import React from 'react';
import { useLoadTest } from '../hooks/useLoadTest';

function formatTime(timestamp: number): string {
  return new Date(timestamp).toLocaleString();
}

function gradeColor(grade: string): string {
  switch (grade) {
    case 'A+':
    case 'A':
      return '#3fb950';
    case 'B':
      return '#58a6ff';
    case 'C':
      return '#d29922';
    case 'D':
      return '#db6d28';
    default:
      return '#f85149';
  }
}

export const LoadTestPanel = React.memo(function LoadTestPanel() {
  const { status, result, history, error, runTest } = useLoadTest();

  return (
    <div className="load-test-panel">
      <div className="load-test-header">
        <h2>Network Quality Test</h2>
        <button
          className="btn btn-primary"
          onClick={runTest}
          disabled={status === 'running'}
        >
          {status === 'running' ? 'Running...' : 'Run Speed Test'}
        </button>
      </div>

      {status === 'running' && (
        <div className="load-test-progress">
          <div className="spinner" />
          Testing network quality... this takes about 25 seconds.
        </div>
      )}

      {error && <div className="load-test-error">{error}</div>}

      {result && status !== 'running' && (
        <div className="load-test-result">
          <div className="load-test-grade" style={{ borderColor: gradeColor(result.grade) }}>
            <span className="grade-label">Bufferbloat</span>
            <span className="grade-value" style={{ color: gradeColor(result.grade) }}>
              {result.grade}
            </span>
          </div>
          <div className="load-test-metrics">
            <div className="metric">
              <span className="metric-label">Download</span>
              <span className="metric-value">{result.downloadMbps.toFixed(1)} Mbps</span>
            </div>
            <div className="metric">
              <span className="metric-label">Upload</span>
              <span className="metric-value">{result.uploadMbps.toFixed(1)} Mbps</span>
            </div>
            <div className="metric">
              <span className="metric-label">Idle Latency</span>
              <span className="metric-value">{result.idleLatencyMs.toFixed(1)} ms</span>
            </div>
            <div className="metric">
              <span className="metric-label">Download Latency</span>
              <span className="metric-value">{result.downloadLoadedLatencyMs.toFixed(1)} ms</span>
            </div>
            <div className="metric">
              <span className="metric-label">Upload Latency</span>
              <span className="metric-value">{result.uploadLoadedLatencyMs.toFixed(1)} ms</span>
            </div>
            <div className="metric">
              <span className="metric-label">Jitter</span>
              <span className="metric-value">{result.idleJitterMs.toFixed(1)} ms</span>
            </div>
          </div>
        </div>
      )}

      {history.length > 0 && (
        <div className="load-test-history">
          <h3>History</h3>
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th>Grade</th>
                <th>Down</th>
                <th>Up</th>
                <th>Idle</th>
                <th>Loaded</th>
              </tr>
            </thead>
            <tbody>
              {history.slice(0, 10).map((r) => (
                <tr key={r.timestamp}>
                  <td>{formatTime(r.timestamp)}</td>
                  <td style={{ color: gradeColor(r.grade), fontWeight: 600 }}>{r.grade}</td>
                  <td>{r.downloadMbps.toFixed(1)}</td>
                  <td>{r.uploadMbps.toFixed(1)}</td>
                  <td>{r.idleLatencyMs.toFixed(0)} ms</td>
                  <td>{Math.max(r.downloadLoadedLatencyMs, r.uploadLoadedLatencyMs).toFixed(0)} ms</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
});
