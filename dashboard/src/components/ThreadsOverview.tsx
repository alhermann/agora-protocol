import { useCallback, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getThreads, createThread } from '../api';
import type { ThreadSummary } from '../api';
import { useToast } from './Toast';
import type { ViewState } from '../types';

function formatRelative(iso: string): string {
  const diff = (Date.now() - new Date(iso).getTime()) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function ThreadsOverview({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data, refresh } = usePolling<{ count: number; threads: ThreadSummary[] }>(useCallback(() => getThreads(), []), 10000);
  const [newTitle, setNewTitle] = useState('');
  const [creating, setCreating] = useState(false);
  const { toast } = useToast();

  const threads = data?.threads ?? [];
  const active = threads.filter(t => !t.closed);
  const closed = threads.filter(t => t.closed);

  const handleCreate = async () => {
    if (!newTitle.trim()) return;
    setCreating(true);
    try {
      const result = await createThread(newTitle.trim());
      setNewTitle('');
      refresh();
      toast('Thread created', 'success');
      onSelect({ type: 'thread', id: result.thread_id });
    } catch {
      toast('Failed to create thread', 'error');
    }
    setCreating(false);
  };

  return (
    <div style={{ padding: 32, height: '100%', overflow: 'auto' }}>
      <h2 style={{ fontSize: 24, fontWeight: 700, color: 'var(--text-bright)', marginBottom: 16 }}>Threads</h2>

      {/* Create new thread */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 24 }}>
        <input
          type="text" placeholder="Start a new discussion..."
          value={newTitle} onChange={e => setNewTitle(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && handleCreate()}
          style={{ flex: 1, maxWidth: 500, padding: '10px 14px', fontSize: 13, background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8, color: 'var(--text)', outline: 'none' }}
        />
        <button onClick={handleCreate} disabled={creating || !newTitle.trim()} style={{
          padding: '10px 20px', fontSize: 13, borderRadius: 8, border: 'none', cursor: 'pointer',
          background: 'var(--accent)', color: '#fff', opacity: creating ? 0.5 : 1,
        }}>{creating ? 'Creating...' : '+ Thread'}</button>
      </div>

      {/* Active threads */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 32 }}>
        {active.map(t => (
          <button key={t.id} onClick={() => onSelect({ type: 'thread', id: t.id })} style={{
            background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10,
            padding: '14px 18px', cursor: 'pointer', textAlign: 'left', transition: 'border-color 0.15s',
            display: 'flex', alignItems: 'center', gap: 12,
          }}
          onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
          onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
          >
            <span style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-bright)', flex: 1 }}>{t.title}</span>
            <span style={{ fontSize: 11, color: 'var(--text-dim)' }}>{t.participant_count} participants</span>
            <span style={{ fontSize: 11, color: 'var(--text-dim)' }}>{formatRelative(t.created_at)}</span>
          </button>
        ))}
        {active.length === 0 && (
          <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>No active threads. Start a discussion above.</div>
        )}
      </div>

      {/* Closed threads */}
      {closed.length > 0 && (
        <>
          <h3 style={{ fontSize: 14, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 8 }}>Closed ({closed.length})</h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {closed.map(t => (
              <button key={t.id} onClick={() => onSelect({ type: 'thread', id: t.id })} style={{
                background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 8,
                padding: '10px 14px', cursor: 'pointer', textAlign: 'left', opacity: 0.6,
              }}>
                <span style={{ fontSize: 13, color: 'var(--text-dim)' }}>{t.title}</span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
