import React, { useCallback, useRef } from 'react';
import type { TimeRange, TimeRangePreset } from '../types';
import { isPresetRange } from '../types';

interface Props {
  value: TimeRange;
  onChange: (range: TimeRange) => void;
}

const RANGES: { label: string; value: TimeRangePreset }[] = [
  { label: '1h', value: '1h' },
  { label: '24h', value: '24h' },
  { label: '7d', value: '7d' },
  { label: '30d', value: '30d' },
];

function toDateInputValue(ts: number): string {
  const d = new Date(ts);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}

function getMinDate(): string {
  return toDateInputValue(Date.now() - 30 * 24 * 60 * 60 * 1000);
}

function getMaxDate(): string {
  return toDateInputValue(Date.now());
}

export const TimeSelector = React.memo(function TimeSelector({ value, onChange }: Props) {
  const dateRef = useRef<HTMLInputElement>(null);

  const handlePreset = useCallback((preset: TimeRangePreset) => {
    if (dateRef.current) dateRef.current.value = '';
    onChange(preset);
  }, [onChange]);

  const handleDateChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    if (!val) return;
    // Parse as local date, get start-of-day timestamp in ms
    const [y, m, d] = val.split('-').map(Number);
    const startOfDay = new Date(y, m - 1, d).getTime();
    onChange({ customDay: startOfDay });
  }, [onChange]);

  const isCustom = !isPresetRange(value);
  const dateValue = isCustom ? toDateInputValue(value.customDay) : '';

  return (
    <div className="time-selector">
      {RANGES.map(r => (
        <button
          key={r.value}
          className={`time-btn ${!isCustom && value === r.value ? 'active' : ''}`}
          onClick={() => handlePreset(r.value)}
        >
          {r.label}
        </button>
      ))}
      <input
        ref={dateRef}
        type="date"
        className={`time-date-input ${isCustom ? 'active' : ''}`}
        value={dateValue}
        min={getMinDate()}
        max={getMaxDate()}
        onChange={handleDateChange}
        title="View a specific day"
      />
    </div>
  );
});
