import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SyncStatus {
  enabled: boolean;
  lastPush: number | null;
  lastError: string | null;
}

export const SyncIndicator = React.memo(function SyncIndicator() {
  const [status, setStatus] = useState<SyncStatus | null>(null);

  useEffect(() => {
    const fetch = () => {
      invoke<SyncStatus>('get_sync_status').then(setStatus).catch(() => {});
    };
    fetch();
    const interval = setInterval(fetch, 30000);
    return () => clearInterval(interval);
  }, []);

  if (!status) return null;

  const lastPushText = status.lastPush
    ? `Last sync: ${new Date(status.lastPush).toLocaleTimeString()}`
    : 'Not synced yet';

  return (
    <span
      style={{ fontSize: 11, color: status.lastError ? '#f85149' : '#8b949e' }}
      title={status.lastError || lastPushText}
    >
      {status.enabled ? (
        <>
          <span style={{
            display: 'inline-block',
            width: 6,
            height: 6,
            borderRadius: '50%',
            background: status.lastError ? '#f85149' : '#3fb950',
            marginRight: 4,
          }} />
          Syncing
        </>
      ) : (
        'Sync off'
      )}
    </span>
  );
});
