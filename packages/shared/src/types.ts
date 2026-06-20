export type ProbeMode = 'icmp' | 'icmp-large' | 'http';

export interface Target {
  id: number;
  address: string;
  label: string;
  active: boolean;
  probeMode: ProbeMode;
}

export interface PresetTarget {
  address: string;
  label: string;
}

export interface Mode {
  id: string;
  label: string;
  probeMode: ProbeMode;
  targets: PresetTarget[];
}

const ICMP_TARGETS: PresetTarget[] = [
  { address: '1.1.1.1', label: 'Cloudflare' },
  { address: '8.8.8.8', label: 'Google DNS' },
  { address: '9.9.9.9', label: 'Quad9' },
  { address: '208.67.222.222', label: 'OpenDNS' },
];

export const MODES: Mode[] = [
  { id: 'icmp-32', label: 'ICMP Ping (32B)', probeMode: 'icmp', targets: ICMP_TARGETS },
  { id: 'icmp-1472', label: 'ICMP Ping (1472B MTU)', probeMode: 'icmp-large', targets: ICMP_TARGETS },
  { id: 'http-12k', label: 'HTTP POST (12KB)', probeMode: 'http', targets: [{ address: 'cf-speed-12k', label: 'Cloudflare Speed' }] },
  { id: 'http-100k', label: 'HTTP POST (100KB)', probeMode: 'http', targets: [{ address: 'cf-speed-100k', label: 'Cloudflare Speed' }] },
];

export const ALL_PRESET_ADDRESSES = MODES.flatMap(m => m.targets.map(t => ({ ...t, probeMode: m.probeMode })));

export interface HopStats {
  hop: number;
  ip: string;
  hostname: string | null;
  lossPct: number;
  sent: number;
  recv: number;
  best: number;
  avg: number;
  worst: number;
  last: number;
}

export interface ChartPoint {
  timestamp: number;
  [hopKey: string]: number;
}

export type TimeRangePreset = '1h' | '24h' | '7d' | '30d';
export type TimeRange = TimeRangePreset | { customDay: number };

export function isPresetRange(range: TimeRange): range is TimeRangePreset {
  return typeof range === 'string';
}

const TIME_RANGE_MS: Record<TimeRangePreset, number> = {
  '1h': 60 * 60 * 1000,
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
};

export function timeRangeDurationMs(range: TimeRange): number {
  if (isPresetRange(range)) return TIME_RANGE_MS[range];
  return 24 * 60 * 60 * 1000; // custom day = 24h
}

export function timeRangeQueryWindow(range: TimeRange): [number, number] {
  if (isPresetRange(range)) {
    const now = Date.now();
    return [now - TIME_RANGE_MS[range], now];
  }
  return [range.customDay, range.customDay + 24 * 60 * 60 * 1000];
}

export interface LoadTestResult {
  timestamp: number;
  idleLatencyMs: number;
  idleJitterMs: number;
  downloadMbps: number;
  downloadLoadedLatencyMs: number;
  uploadMbps: number;
  uploadLoadedLatencyMs: number;
  grade: string;
}

export interface DashboardData {
  target: string;
  hops: HopStats[];
  lossChart: ChartPoint[];
  latencyChart: ChartPoint[];
}

export interface HopAttribution {
  hop: number;
  ip: string;
  lossPct: number;
  scope: 'local-gateway' | 'beyond-gateway';
}

export interface TargetReport {
  address: string;
  label: string;
  probeMode: ProbeMode;
  samples: number;
  lossPct: number;
  avgLatencyMs: number;
  worstLatencyMs: number;
  availabilityPct: number;
  firstLossHop: HopAttribution | null;
}

export interface OutageEvent {
  start: number;
  end: number;
  durationSecs: number;
  peakLossPct: number;
}

export interface ReportBucket {
  timestamp: number;
  sent: number;
  lossPct: number;
}

export interface ReportData {
  generatedAt: number;
  periodStart: number;
  periodEnd: number;
  deviceName: string;
  platform: string;
  overallLossPct: number;
  totalSamples: number;
  targets: TargetReport[];
  outages: OutageEvent[];
  lossSeries: ReportBucket[];
  loadTests: LoadTestResult[];
  probeIntervalSecs: number;
}
