import React from 'react';
import type { ChartPoint, TimeRange } from '../types';
import { RouteChart } from './RouteChart';

interface Props {
  data: ChartPoint[];
  hopCount: number;
  timeRange: TimeRange;
  height?: number;
  helperText?: string;
  viewport?: [number, number] | null;
  onViewportChange?: (viewport: [number, number] | null) => void;
}

export const LatencyChart = React.memo(function LatencyChart({
  data,
  hopCount,
  timeRange,
  height,
  helperText,
  viewport,
  onViewportChange,
}: Props) {
  return (
    <RouteChart
      title="Latency"
      data={data}
      hopCount={hopCount}
      timeRange={timeRange}
      unitLabel=" ms"
      emptyLabel="Collecting latency samples..."
      height={height}
      helperText={helperText}
      viewport={viewport}
      onViewportChange={onViewportChange}
      yAxisTickFormatter={(value) => `${value}ms`}
    />
  );
});
