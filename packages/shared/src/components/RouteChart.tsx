import React, { useMemo, useState } from 'react';
import {
  Brush,
  CartesianGrid,
  Line,
  LineChart,
  ReferenceArea,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import type { ChartPoint, TimeRange } from '../types';
import { isPresetRange, timeRangeQueryWindow } from '../types';

const HOP_COLORS = [
  '#58a6ff', '#3fb950', '#d29922', '#f85149', '#bc8cff',
  '#79c0ff', '#56d364', '#e3b341', '#ff7b72', '#d2a8ff',
  '#39d353', '#f0883e', '#8b949e', '#ff9bce', '#a5d6ff',
];

type Viewport = [number, number] | null;

interface Props {
  title: string;
  data: ChartPoint[];
  hopCount: number;
  timeRange: TimeRange;
  unitLabel: string;
  emptyLabel: string;
  height?: number;
  helperText?: string;
  viewport?: Viewport;
  onViewportChange?: (viewport: Viewport) => void;
  yAxisDomain?: [number | 'auto', number | 'auto'];
  yAxisTickFormatter?: (value: number) => string;
}

function formatTick(ts: number, timeRange: TimeRange): string {
  const d = new Date(ts);
  if (isPresetRange(timeRange) && (timeRange === '7d' || timeRange === '30d')) {
    return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function formatTooltipTimestamp(ts: number): string {
  return new Date(ts).toLocaleString([], {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function isViewportEqual(a: Viewport, b: Viewport): boolean {
  if (a === null && b === null) {
    return true;
  }
  if (a === null || b === null) {
    return false;
  }
  return a[0] === b[0] && a[1] === b[1];
}

function resolveViewport(viewport: Viewport | undefined, timeRange: TimeRange): [number, number] {
  if (viewport) {
    return viewport[0] <= viewport[1] ? viewport : [viewport[1], viewport[0]];
  }
  return timeRangeQueryWindow(timeRange);
}

function findBrushIndexes(data: ChartPoint[], viewport: [number, number]) {
  if (data.length === 0) {
    return { startIndex: 0, endIndex: 0 };
  }

  const [start, end] = viewport;
  let startIndex = data.findIndex(point => point.timestamp >= start);
  if (startIndex === -1) {
    startIndex = 0;
  }

  let endIndex = data.length - 1;
  while (endIndex > startIndex && data[endIndex].timestamp > end) {
    endIndex -= 1;
  }

  return { startIndex, endIndex: Math.max(startIndex, endIndex) };
}

function getActiveTimestamp(state: unknown): number | null {
  if (
    typeof state === 'object' &&
    state !== null &&
    'activeLabel' in state &&
    typeof (state as { activeLabel?: unknown }).activeLabel === 'number'
  ) {
    return (state as { activeLabel: number }).activeLabel;
  }
  return null;
}

export const RouteChart = React.memo(function RouteChart({
  title,
  data,
  hopCount,
  timeRange,
  unitLabel,
  emptyLabel,
  height = 220,
  helperText = 'Drag in the plot or use the brush to zoom.',
  viewport,
  onViewportChange,
  yAxisDomain,
  yAxisTickFormatter,
}: Props) {
  const [selectionStart, setSelectionStart] = useState<number | null>(null);
  const [selectionEnd, setSelectionEnd] = useState<number | null>(null);

  const hopKeys = useMemo(
    () => Array.from({ length: hopCount }, (_, i) => `hop${i + 1}`),
    [hopCount],
  );
  const visibleViewport = resolveViewport(viewport, timeRange);
  const brushIndexes = useMemo(
    () => findBrushIndexes(data, visibleViewport),
    [data, visibleViewport],
  );

  const clearSelection = () => {
    setSelectionStart(null);
    setSelectionEnd(null);
  };

  const commitViewport = (nextViewport: Viewport) => {
    if (!onViewportChange) {
      return;
    }

    const normalized = nextViewport && nextViewport[0] === nextViewport[1]
      ? null
      : nextViewport;

    if (isViewportEqual(normalized, viewport ?? null)) {
      return;
    }

    onViewportChange(normalized);
  };

  const handleBrushChange = (next: { startIndex?: number; endIndex?: number }) => {
    if (!onViewportChange || data.length === 0) {
      return;
    }

    const startIndex = next.startIndex ?? 0;
    const endIndex = next.endIndex ?? (data.length - 1);
    const start = data[startIndex]?.timestamp;
    const end = data[endIndex]?.timestamp;

    if (start === undefined || end === undefined) {
      return;
    }

    const fullViewport: Viewport = [data[0].timestamp, data[data.length - 1].timestamp];
    const nextViewport: Viewport = [Math.min(start, end), Math.max(start, end)];
    commitViewport(isViewportEqual(nextViewport, fullViewport) ? null : nextViewport);
  };

  const handleMouseDown = (state: unknown) => {
    if (!onViewportChange || data.length < 2) {
      return;
    }

    const timestamp = getActiveTimestamp(state);
    if (timestamp === null) {
      return;
    }

    setSelectionStart(timestamp);
    setSelectionEnd(timestamp);
  };

  const handleMouseMove = (state: unknown) => {
    if (selectionStart === null) {
      return;
    }

    const timestamp = getActiveTimestamp(state);
    if (timestamp !== null) {
      setSelectionEnd(timestamp);
    }
  };

  const handleMouseUp = () => {
    if (selectionStart === null || selectionEnd === null) {
      clearSelection();
      return;
    }

    const start = Math.min(selectionStart, selectionEnd);
    const end = Math.max(selectionStart, selectionEnd);
    clearSelection();

    if (end - start < 1000) {
      return;
    }

    commitViewport([start, end]);
  };

  const handleReset = () => {
    clearSelection();
    commitViewport(null);
  };

  const tooltipFormatter = (value: number | string | undefined, name: string | undefined) => {
    const numericValue = typeof value === 'number' ? value : Number(value ?? 0);
    return [`${numericValue}${unitLabel}`, name ?? ''];
  };

  return (
    <div className="chart-card">
      <div className="chart-card-header">
        <div>
          <h3>{title}</h3>
          {helperText && <p className="chart-card-meta">{helperText}</p>}
        </div>
        {onViewportChange && viewport && (
          <button type="button" className="chart-reset" onClick={handleReset}>
            Reset Zoom
          </button>
        )}
      </div>

      {data.length === 0 ? (
        <div className="loading chart-empty" style={{ minHeight: height }}>{emptyLabel}</div>
      ) : (
        <ResponsiveContainer width="100%" height={height}>
          <LineChart
            data={data}
            syncId="route-monitor"
            accessibilityLayer={false}
            margin={{ top: 10, right: 8, bottom: 8, left: -14 }}
            onMouseDown={handleMouseDown}
            onMouseMove={handleMouseMove}
            onMouseUp={handleMouseUp}
            onDoubleClick={handleReset}
          >
            <CartesianGrid strokeDasharray="3 3" stroke="#1c2b38" />
            <XAxis
              dataKey="timestamp"
              type="number"
              domain={visibleViewport}
              allowDataOverflow
              tickFormatter={(ts: number) => formatTick(ts, timeRange)}
              stroke="#5f7888"
              fontSize={11}
              tickCount={6}
            />
            <YAxis
              domain={yAxisDomain}
              stroke="#5f7888"
              fontSize={11}
              tickFormatter={yAxisTickFormatter}
            />
            <Tooltip
              isAnimationActive={false}
              contentStyle={{
                background: '#0d1720',
                border: '1px solid rgba(123, 171, 203, 0.28)',
                borderRadius: 12,
                fontSize: 12,
              }}
              labelFormatter={(label) => formatTooltipTimestamp(Number(label))}
              formatter={tooltipFormatter}
            />
            {selectionStart !== null && selectionEnd !== null && (
              <ReferenceArea
                x1={selectionStart}
                x2={selectionEnd}
                strokeOpacity={0}
                fill="rgba(76, 194, 255, 0.12)"
              />
            )}
            {hopKeys.map((key, i) => (
              <Line
                key={key}
                type="monotone"
                dataKey={key}
                stroke={HOP_COLORS[i % HOP_COLORS.length]}
                strokeWidth={1.6}
                dot={false}
                activeDot={{ r: 3, strokeWidth: 0 }}
                connectNulls
                name={`Hop ${i + 1}`}
              />
            ))}
            {data.length > 1 && (
              <Brush
                dataKey="timestamp"
                height={28}
                stroke="#4cc2ff"
                travellerWidth={12}
                startIndex={brushIndexes.startIndex}
                endIndex={brushIndexes.endIndex}
                tickFormatter={(ts: number) => formatTick(ts, timeRange)}
                onChange={handleBrushChange}
              />
            )}
          </LineChart>
        </ResponsiveContainer>
      )}
    </div>
  );
});
