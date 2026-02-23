import React from 'react';
import type { TimeRange } from '../types';

interface Props {
  value: TimeRange;
  onChange: (range: TimeRange) => void;
}

const RANGES: { label: string; value: TimeRange }[] = [
  { label: '1h', value: '1h' },
  { label: '24h', value: '24h' },
  { label: '7d', value: '7d' },
  { label: '30d', value: '30d' },
];

export const TimeSelector = React.memo(function TimeSelector({ value, onChange }: Props) {
  return (
    <div className="time-selector">
      {RANGES.map(r => (
        <button
          key={r.value}
          className={`time-btn ${value === r.value ? 'active' : ''}`}
          onClick={() => onChange(r.value)}
        >
          {r.label}
        </button>
      ))}
    </div>
  );
});
