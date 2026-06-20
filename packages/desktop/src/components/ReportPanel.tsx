import React, { useEffect, useState } from 'react';
import { TimeSelector } from '@netmon/shared';
import type { ReportData, TimeRange } from '@netmon/shared';
import * as api from '../api';

function fmtDateTime(ts: number): string {
  return new Date(ts).toLocaleString();
}

function fmtDuration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function probeModeLabel(mode: string): string {
  if (mode === 'http') return 'HTTP upload';
  if (mode === 'icmp-large') return 'ICMP 1472B';
  return 'ICMP 32B';
}

function buildReportText(data: ReportData): string {
  const lines: string[] = [];
  const L = (s = '') => lines.push(s);

  L('NETWORK QUALITY REPORT');
  L('='.repeat(64));
  L(`Generated:         ${fmtDateTime(data.generatedAt)}`);
  L(`Device:            ${data.deviceName} (${data.platform})`);
  L(`Monitoring period: ${fmtDateTime(data.periodStart)}  to  ${fmtDateTime(data.periodEnd)}`);
  L(`Probe frequency:   every ${data.probeIntervalSecs}s per target`);
  L('');

  L('SUMMARY');
  L('-'.repeat(64));
  L(`Overall packet loss (all targets): ${data.overallLossPct}%`);
  L(`Total probe samples analyzed:      ${data.totalSamples.toLocaleString()}`);
  L('');

  L('PER-TARGET RESULTS');
  L('-'.repeat(64));
  for (const t of data.targets) {
    L(`${t.label} (${t.address}) [${probeModeLabel(t.probeMode)}]`);
    L(`  Packet loss: ${t.lossPct}%    Availability: ${t.availabilityPct}%`);
    L(`  Avg latency: ${t.avgLatencyMs} ms    Worst: ${t.worstLatencyMs} ms`);
    L(`  Samples:     ${t.samples.toLocaleString()}`);
    if (t.firstLossHop) {
      const h = t.firstLossHop;
      const scope = h.scope === 'local-gateway'
        ? 'your local network / router'
        : 'beyond your local network (likely ISP or upstream)';
      L(`  Loss first appears at hop ${h.hop} (${h.ip}) - ${scope}, ${h.lossPct}% loss`);
    }
    L('');
  }

  L('OUTAGE EVENTS (sustained loss >= 5%, local time)');
  L('-'.repeat(64));
  if (data.outages.length === 0) {
    L('No sustained outage events detected.');
  } else {
    for (const o of data.outages) {
      L(`${fmtDateTime(o.start)}  ->  ${fmtDateTime(o.end)}  (${fmtDuration(o.durationSecs)}), peak loss ${o.peakLossPct}%`);
    }
  }
  L('');

  L('TIME-OF-DAY PATTERN (local time)');
  L('-'.repeat(64));
  if (data.lossSeries.length === 0) {
    L('Not enough data for a time-of-day breakdown.');
  } else {
    const hourSent = new Array<number>(24).fill(0);
    const hourLoss = new Array<number>(24).fill(0);
    for (const b of data.lossSeries) {
      const hour = new Date(b.timestamp).getHours();
      hourSent[hour] += b.sent;
      hourLoss[hour] += (b.sent * b.lossPct) / 100;
    }
    const hourPct = hourSent.map((sent, i) => (sent > 0 ? (hourLoss[i] / sent) * 100 : 0));
    const peak = hourPct.reduce((best, value, i, arr) => (value > arr[best] ? i : best), 0);
    if (hourSent[peak] > 0 && hourPct[peak] > 0) {
      const next = String((peak + 1) % 24).padStart(2, '0');
      L(`Highest packet loss occurs around ${String(peak).padStart(2, '0')}:00-${next}:00 (${hourPct[peak].toFixed(1)}%).`);
      L('');
    }
    for (let h = 0; h < 24; h++) {
      if (hourSent[h] === 0) continue;
      const bar = '#'.repeat(Math.round(hourPct[h] / 2));
      L(`${String(h).padStart(2, '0')}:00  ${hourPct[h].toFixed(1).padStart(5)}%  ${bar}`);
    }
  }
  L('');

  L('SPEED / BUFFERBLOAT TESTS');
  L('-'.repeat(64));
  if (data.loadTests.length === 0) {
    L('No speed tests recorded in this period.');
  } else {
    for (const lt of data.loadTests) {
      L(`${fmtDateTime(lt.timestamp)}: down ${lt.downloadMbps} Mbps / up ${lt.uploadMbps} Mbps, ` +
        `idle ${lt.idleLatencyMs} ms, loaded ${lt.downloadLoadedLatencyMs}/${lt.uploadLoadedLatencyMs} ms (grade ${lt.grade})`);
    }
  }
  L('');

  L('METHODOLOGY');
  L('-'.repeat(64));
  L('This report was generated automatically by NetMon from continuous active network');
  L(`measurements. Each monitored target was probed approximately every ${data.probeIntervalSecs}`);
  L('seconds. Packet loss is measured end-to-end at the destination. Per-hop attribution');
  L('uses ICMP TTL probes; intermediate-hop loss is indicative only, as routers may');
  L('rate-limit or deprioritize TTL-expired replies. Measurements reflect network-layer');
  L('quality (packet loss, latency), not wireless signal strength.');

  return lines.join('\n');
}

export function ReportPanel({ onClose }: { onClose: () => void }) {
  const [timeRange, setTimeRange] = useState<TimeRange>('7d');
  const [report, setReport] = useState<string>('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api.generateReport(timeRange)
      .then(data => {
        if (!cancelled) {
          setReport(buildReportText(data));
        }
      })
      .catch(err => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [timeRange]);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(report);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setError('Could not copy automatically. Select the text and copy manually.');
    }
  };

  return (
    <div className="modal-overlay report-overlay">
      <div className="modal report-card">
        <header className="report-header report-actions">
          <div>
            <h2>Network Quality Report</h2>
            <p className="report-subtitle">For ISP / FCC complaints and personal records.</p>
          </div>
          <TimeSelector value={timeRange} onChange={setTimeRange} />
        </header>

        {error && <div className="error-banner">{error}</div>}

        {loading ? (
          <div className="loading loading-panel">
            <div className="spinner" />
            Analyzing observed data...
          </div>
        ) : (
          <pre className="report-text report-print">{report}</pre>
        )}

        <footer className="report-footer report-actions">
          <button className="btn" onClick={onClose}>Close</button>
          <button className="btn" onClick={() => window.print()} disabled={loading || !report}>Print / Save PDF</button>
          <button className="btn btn-primary" onClick={handleCopy} disabled={loading || !report}>
            {copied ? 'Copied' : 'Copy as text'}
          </button>
        </footer>
      </div>
    </div>
  );
}
