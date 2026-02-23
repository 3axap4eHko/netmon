export interface Target {
  id: number;
  address: string;
  label: string;
  active: boolean;
}

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

export type TimeRange = '1h' | '24h' | '7d' | '30d';

export interface DashboardData {
  target: string;
  hops: HopStats[];
  lossChart: ChartPoint[];
  latencyChart: ChartPoint[];
}
