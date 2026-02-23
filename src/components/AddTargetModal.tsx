import React, { useState } from 'react';

interface Props {
  onAdd: (address: string, label: string) => Promise<void> | void;
  onClose: () => void;
}

export const AddTargetModal = React.memo(function AddTargetModal({ onAdd, onClose }: Props) {
  const [address, setAddress] = useState('');
  const [label, setLabel] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!address.trim() || submitting) return;

    setSubmitting(true);
    setError(null);
    try {
      await onAdd(address.trim(), label.trim() || address.trim());
    } catch (err) {
      setError(typeof err === 'string' ? err : 'Unable to add target.');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <h2>Add Monitoring Target</h2>
        <form onSubmit={handleSubmit}>
          <input
            type="text"
            placeholder="IP address or hostname (e.g. 8.8.8.8)"
            value={address}
            onChange={e => setAddress(e.target.value)}
            autoFocus
          />
          <input
            type="text"
            placeholder="Label (optional, e.g. Google DNS)"
            value={label}
            onChange={e => setLabel(e.target.value)}
          />
          {error && <div className="form-error">{error}</div>}
          <div className="modal-buttons">
            <button type="button" className="btn" onClick={onClose} disabled={submitting}>Cancel</button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? 'Adding...' : 'Add'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
});
