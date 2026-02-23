import React from 'react';
import {
  LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend,
} from 'recharts';
import type { ChartPoint } from '../types';

const HOP_COLORS = [
  '#58a6ff', '#3fb950', '#d29922', '#f85149', '#bc8cff',
  '#79c0ff', '#56d364', '#e3b341', '#ff7b72', '#d2a8ff',
  '#39d353', '#f0883e', '#8b949e', '#ff9bce', '#a5d6ff',
];

interface Props {
  data: ChartPoint[];
  hopCount: number;
}

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export const LossChart = React.memo(function LossChart({ data, hopCount }: Props) {
  const hopKeys = Array.from({ length: hopCount }, (_, i) => `hop${i + 1}`);

  return (
    <div className="chart-card">
      <h3>Packet Loss Count</h3>
      {data.length === 0 ? (
        <div className="loading" style={{ height: 220 }}>Collecting data...</div>
      ) : (
        <ResponsiveContainer width="100%" height={220}>
          <LineChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: -10 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#21262d" />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTime}
              stroke="#484f58"
              fontSize={10}
            />
            <YAxis
              domain={[0, 'auto']}
              stroke="#484f58"
              fontSize={10}
              tickFormatter={(v: number) => `${v}`}
            />
            <Tooltip
              contentStyle={{ background: '#161b22', border: '1px solid #30363d', borderRadius: 6, fontSize: 12 }}
              labelFormatter={(label) => new Date(Number(label)).toLocaleString()}
              formatter={(value, name) => [`${value ?? 0}`, name ?? '']}
            />
            <Legend wrapperStyle={{ fontSize: 11 }} />
            {hopKeys.map((key, i) => (
              <Line
                key={key}
                type="monotone"
                dataKey={key}
                stroke={HOP_COLORS[i % HOP_COLORS.length]}
                strokeWidth={1.5}
                dot={false}
                connectNulls
                name={`Hop ${i + 1}`}
              />
            ))}
          </LineChart>
        </ResponsiveContainer>
      )}
    </div>
  );
});
