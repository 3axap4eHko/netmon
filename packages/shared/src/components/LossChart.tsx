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

export const LossChart = React.memo(function LossChart({
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
      title="Loss Events"
      data={data}
      hopCount={hopCount}
      timeRange={timeRange}
      unitLabel=""
      emptyLabel="Collecting packet loss samples..."
      height={height}
      helperText={helperText}
      viewport={viewport}
      onViewportChange={onViewportChange}
      yAxisDomain={[0, 'auto']}
      yAxisTickFormatter={(value) => `${value}`}
    />
  );
});
