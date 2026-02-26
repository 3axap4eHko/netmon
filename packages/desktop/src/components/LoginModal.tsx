import React, { useState } from 'react';

interface Props {
  onLogin: (email: string, password: string) => Promise<void>;
  onRegister: (email: string, password: string) => Promise<void>;
  onOAuth: (provider: string) => Promise<void>;
  onClose: () => void;
}

export const LoginModal = React.memo(function LoginModal({ onLogin, onRegister, onOAuth, onClose }: Props) {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [isRegister, setIsRegister] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!email.trim() || !password.trim() || submitting) return;

    setSubmitting(true);
    setError(null);
    try {
      if (isRegister) {
        await onRegister(email.trim(), password);
      } else {
        await onLogin(email.trim(), password);
      }
      onClose();
    } catch (err) {
      setError(typeof err === 'string' ? err : 'Authentication failed.');
    } finally {
      setSubmitting(false);
    }
  };

  const handleGoogleLogin = async () => {
    try {
      await onOAuth('google');
      onClose();
    } catch (err) {
      setError(typeof err === 'string' ? err : 'OAuth failed.');
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <h2>{isRegister ? 'Create Account' : 'Sign In'}</h2>
        <p style={{ fontSize: 13, color: '#8b949e', marginBottom: 16 }}>
          Sign in to sync your monitoring data to the cloud.
        </p>

        <button
          className="btn"
          style={{ width: '100%', marginBottom: 16 }}
          onClick={handleGoogleLogin}
          disabled={submitting}
        >
          Continue with Google
        </button>

        <div style={{ textAlign: 'center', fontSize: 12, color: '#484f58', margin: '12px 0' }}>or</div>

        <form onSubmit={handleSubmit}>
          <input
            type="email"
            placeholder="Email"
            value={email}
            onChange={e => setEmail(e.target.value)}
          />
          <input
            type="password"
            placeholder="Password"
            value={password}
            onChange={e => setPassword(e.target.value)}
          />
          {error && <div className="form-error">{error}</div>}
          <div className="modal-buttons">
            <button type="button" className="btn" onClick={onClose} disabled={submitting}>Cancel</button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? 'Please wait...' : isRegister ? 'Register' : 'Sign In'}
            </button>
          </div>
        </form>

        <div style={{ textAlign: 'center', marginTop: 12 }}>
          <button
            style={{ background: 'none', border: 'none', color: '#58a6ff', cursor: 'pointer', fontSize: 12 }}
            onClick={() => setIsRegister(!isRegister)}
          >
            {isRegister ? 'Already have an account? Sign in' : "Don't have an account? Register"}
          </button>
        </div>
      </div>
    </div>
  );
});
