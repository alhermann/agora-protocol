import { useCallback } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getStatus } from '../api';
import type { StatusResponse } from '../types';

export function HeaderBar({ onHome }: { onHome?: () => void }) {
  const fetchStatus = useCallback(() => getStatus(), []);
  const { data: status, error } = usePolling<StatusResponse>(fetchStatus, 10000);

  const online = !error && status?.running;
  const name = status?.node_name ?? '...';

  return (
    <header className="header-bar">
      <div className="header-left">
        <button
          onClick={onHome}
          style={{
            background: 'none', border: 'none', cursor: 'pointer', padding: 0,
            display: 'flex', alignItems: 'center', gap: 10,
          }}
        >
          <span style={{
            fontSize: 18, fontWeight: 800, letterSpacing: '0.08em',
            background: 'linear-gradient(135deg, var(--accent), #a78bfa)',
            WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent',
          }}>AGORA</span>
        </button>
      </div>
      <div className="header-right">
        <span className={`status-dot-sm ${online ? 'online' : 'offline'}`} />
        <span style={{ fontSize: 13, color: 'var(--text-bright)', fontWeight: 600 }}>{name}</span>
        <span style={{
          fontSize: 11,
          color: online ? 'var(--green)' : 'var(--text-dim)',
          textTransform: 'uppercase' as const,
          letterSpacing: '0.04em',
          fontWeight: 500,
        }}>
          {online ? 'online' : 'offline'}
        </span>
      </div>
    </header>
  );
}
