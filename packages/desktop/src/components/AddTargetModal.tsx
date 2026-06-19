import React, { useState } from 'react';
import type { ProbeMode } from '@netmon/shared';

interface Props {
  onAdd: (address: string, label: string, probeMode: ProbeMode) => Promise<void> | void;
  onClose: () => void;
}

const PROBE_OPTIONS: Array<{ value: ProbeMode; label: string; description: string }> = [
  { value: 'icmp', label: 'ICMP 32B', description: 'Standard packet monitor' },
  { value: 'icmp-large', label: 'ICMP 1472B', description: 'MTU-sized packet monitor' },
];

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === 'string' && error.trim()) {
    return error;
  }
  return 'Unable to add target.';
}

export const AddTargetModal = React.memo(function AddTargetModal({ onAdd, onClose }: Props) {
  const [address, setAddress] = useState('');
  const [label, setLabel] = useState('');
  const [probeMode, setProbeMode] = useState<ProbeMode>('icmp');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!address.trim() || submitting) {
      return;
    }

    setSubmitting(true);
    setError(null);

    try {
      await onAdd(address.trim(), label.trim() || address.trim(), probeMode);
      onClose();
    } catch (nextError) {
      setError(getErrorMessage(nextError));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal modal-wide" onClick={event => event.stopPropagation()}>
        <h2>Add Monitoring Target</h2>
        <p className="modal-copy">
          Add an IPv4 address or hostname. Built-in HTTP upload probes can be restored from the
          dashboard if you remove them.
        </p>

        <form onSubmit={handleSubmit} className="modal-form">
          <label className="field-group">
            <span>Address or Hostname</span>
            <input
              type="text"
              placeholder="8.8.8.8 or dns.google"
              value={address}
              onChange={event => setAddress(event.target.value)}
              autoFocus
            />
          </label>

          <label className="field-group">
            <span>Label</span>
            <input
              type="text"
              placeholder="Optional display name"
              value={label}
              onChange={event => setLabel(event.target.value)}
            />
          </label>

          <div className="field-group">
            <span>Probe Profile</span>
            <div className="probe-grid">
              {PROBE_OPTIONS.map(option => (
                <button
                  key={option.value}
                  type="button"
                  className={`probe-option ${probeMode === option.value ? 'active' : ''}`}
                  onClick={() => setProbeMode(option.value)}
                >
                  <strong>{option.label}</strong>
                  <small>{option.description}</small>
                </button>
              ))}
            </div>
          </div>

          {error && <div className="form-error">{error}</div>}

          <div className="modal-buttons">
            <button type="button" className="btn" onClick={onClose} disabled={submitting}>
              Cancel
            </button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? 'Adding...' : 'Add Target'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
});
