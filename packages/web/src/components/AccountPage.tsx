import React, { useState } from 'react';
import * as api from '../api';

interface Props {
  email: string;
  plan: string;
  onLogout: () => void;
}

export function AccountPage({ email, plan, onLogout }: Props) {
  const [loading, setLoading] = useState(false);

  const handleUpgrade = async () => {
    setLoading(true);
    try {
      const { url } = await api.createCheckoutSession();
      window.location.href = url;
    } catch (err) {
      console.error('Failed to create checkout session:', err);
    } finally {
      setLoading(false);
    }
  };

  const handleManageBilling = async () => {
    setLoading(true);
    try {
      const { url } = await api.createPortalSession();
      window.location.href = url;
    } catch (err) {
      console.error('Failed to create portal session:', err);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="app">
      <div className="header">
        <h1>Account</h1>
      </div>

      <div style={{ maxWidth: 400 }}>
        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, color: '#8b949e', marginBottom: 4 }}>Email</div>
          <div>{email}</div>
        </div>

        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, color: '#8b949e', marginBottom: 4 }}>Plan</div>
          <div style={{ textTransform: 'capitalize' }}>{plan}</div>
        </div>

        {plan === 'free' ? (
          <button className="btn btn-primary" onClick={handleUpgrade} disabled={loading}>
            {loading ? 'Loading...' : 'Upgrade to Pro ($3/mo)'}
          </button>
        ) : (
          <button className="btn" onClick={handleManageBilling} disabled={loading}>
            {loading ? 'Loading...' : 'Manage Billing'}
          </button>
        )}

        <div style={{ marginTop: 24 }}>
          <button className="btn" onClick={onLogout}>Sign Out</button>
        </div>
      </div>
    </div>
  );
}
