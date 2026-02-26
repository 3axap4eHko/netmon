import React from 'react';

interface Props {
  email: string;
  plan: string;
  onLogout: () => void;
  onClose: () => void;
}

export const AccountPage = React.memo(function AccountPage({ email, plan, onLogout, onClose }: Props) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <h2>Account</h2>

        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, color: '#8b949e', marginBottom: 4 }}>Email</div>
          <div>{email}</div>
        </div>

        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, color: '#8b949e', marginBottom: 4 }}>Plan</div>
          <div style={{ textTransform: 'capitalize' }}>{plan}</div>
        </div>

        {plan === 'free' && (
          <div style={{ marginBottom: 16, padding: 12, background: '#0d1117', borderRadius: 6, fontSize: 13 }}>
            Upgrade to <strong>Pro ($3/mo)</strong> for 30-day cloud data retention.
          </div>
        )}

        <div className="modal-buttons">
          <button
            className="btn"
            onClick={() => { onLogout(); onClose(); }}
            style={{ color: '#f85149' }}
          >
            Sign Out
          </button>
          <button className="btn" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
});
