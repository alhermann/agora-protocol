import { useCallback, useEffect, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import {
  getConversations,
  getProjects,
  getProjectTasks,
  getStatus,
  listConsumers,
} from '../api';
import type {
  ConsumersResponse,
  ConversationsResponse,
  ProjectsResponse,
  StatusResponse,
  ViewState,
} from '../types';

function relTime(iso: string): string {
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

interface TaskSnap { total: number; done: number; inProgress: number; blocked: number }

export function WelcomeView({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data: status } = usePolling<StatusResponse>(useCallback(() => getStatus(), []), 10000);
  const { data: projects } = usePolling<ProjectsResponse>(useCallback(() => getProjects(), []), 10000);
  const { data: consumers } = usePolling<ConsumersResponse>(useCallback(() => listConsumers(), []), 10000);
  const { data: convos } = usePolling<ConversationsResponse>(useCallback(() => getConversations(), []), 10000);
  const [tasks, setTasks] = useState<Record<string, TaskSnap>>({});

  const active = projects?.projects.filter(p => p.status === 'active') ?? [];

  useEffect(() => {
    active.forEach(p => {
      getProjectTasks(p.id).then(d => {
        const t = d.tasks ?? [];
        setTasks(prev => ({ ...prev, [p.id]: {
          total: t.length,
          done: t.filter(x => x.status === 'done').length,
          inProgress: t.filter(x => x.status === 'in_progress').length,
          blocked: t.filter(x => x.status === 'blocked').length,
        }}));
      });
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projects]);

  const now = Date.now();
  const online: string[] = [];
  const offline: string[] = [];
  consumers?.consumers.forEach(c => {
    if (c.label === 'http-default' || c.label === status?.node_name || c.label.endsWith('-listener')) return;
    if (now - new Date(c.last_active).getTime() < 60000) online.push(c.label);
    else offline.push(c.label);
  });

  const totalBlocked = Object.values(tasks).reduce((s, t) => s + t.blocked, 0);
  const recent = [...(convos?.conversations ?? [])]
    .sort((a, b) => b.last_message_at.localeCompare(a.last_message_at))
    .slice(0, 6);

  const S = { section: { marginBottom: 28 } as const, heading: { fontSize: 12, color: 'var(--text-dim)', textTransform: 'uppercase' as const, letterSpacing: '0.06em', marginBottom: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center' } as const };

  return (
    <div style={{ padding: 32, height: '100%', overflow: 'auto', maxWidth: 900 }}>

      {/* ── NEEDS ATTENTION ── */}
      {totalBlocked > 0 && (
        <div style={{ background: 'rgba(248,81,73,0.06)', border: '1px solid rgba(248,81,73,0.15)', borderRadius: 8, padding: '10px 16px', marginBottom: 20, fontSize: 13, color: 'var(--red)', display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 16 }}>⚠</span>
          <span><strong>{totalBlocked}</strong> blocked task{totalBlocked > 1 ? 's' : ''} need attention</span>
        </div>
      )}

      {/* ── AGENTS RIGHT NOW ── */}
      <div style={S.section}>
        <div style={S.heading}>
          <span>Agents {online.length > 0 && <span style={{ color: 'var(--green)', fontSize: 11 }}>· {online.length} online</span>}</span>
        </div>
        {online.length > 0 ? (
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: offline.length > 0 ? 8 : 0 }}>
            {online.map(name => {
              const lastMsg = convos?.conversations
                .filter(c => c.participants.includes(name))
                .sort((a, b) => b.last_message_at.localeCompare(a.last_message_at))[0];
              return (
                <button key={name} onClick={() => onSelect({ type: 'agent', name })} style={{
                  background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8,
                  padding: '10px 14px', cursor: 'pointer', textAlign: 'left', transition: 'border-color 0.15s',
                  display: 'flex', flexDirection: 'column', gap: 4, minWidth: 180,
                }}
                onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--green)'; }}
                onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span className="status-dot online" />
                    <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-bright)' }}>{name}</span>
                  </div>
                  {lastMsg && (
                    <div style={{ fontSize: 11, color: 'var(--text-dim)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 200 }}>
                      {lastMsg.preview.slice(0, 50)}
                    </div>
                  )}
                </button>
              );
            })}
          </div>
        ) : (
          <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>No agents connected.</div>
        )}
        {offline.length > 0 && (
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            {offline.map(name => (
              <span key={name} style={{ fontSize: 12, color: 'var(--text-dim)', padding: '4px 10px', background: 'var(--bg-card)', borderRadius: 6 }}>
                <span className="status-dot offline" style={{ marginRight: 4 }} />{name}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* ── PROJECTS ── */}
      <div style={S.section}>
        <div style={S.heading}>
          <span>Projects</span>
          <button onClick={() => onSelect({ type: 'projects' })} style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 11 }}>View all</button>
        </div>
        {active.length > 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {active.map(p => {
              const t = tasks[p.id];
              const pct = t && t.total > 0 ? Math.round((t.done / t.total) * 100) : 0;
              return (
                <button key={p.id} onClick={() => onSelect({ type: 'project', id: p.id })} style={{
                  background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8,
                  padding: '12px 16px', cursor: 'pointer', textAlign: 'left', transition: 'border-color 0.15s',
                }}
                onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
                onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: t && t.total > 0 ? 6 : 0 }}>
                    <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-bright)' }}>{p.name}</span>
                    <div style={{ display: 'flex', gap: 10, fontSize: 11, color: 'var(--text-dim)' }}>
                      {t && t.inProgress > 0 && <span style={{ color: 'var(--accent)' }}>{t.inProgress} in progress</span>}
                      {t && t.blocked > 0 && <span style={{ color: 'var(--red)' }}>{t.blocked} blocked</span>}
                      <span>{p.agent_count} agents</span>
                    </div>
                  </div>
                  {t && t.total > 0 && (
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <div style={{ flex: 1, height: 3, background: 'var(--bg)', borderRadius: 2, overflow: 'hidden' }}>
                        <div style={{ width: `${pct}%`, height: '100%', background: 'var(--green)', borderRadius: 2 }} />
                      </div>
                      <span style={{ fontSize: 10, color: 'var(--text-dim)' }}>{t.done}/{t.total}</span>
                    </div>
                  )}
                </button>
              );
            })}
          </div>
        ) : (
          <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>No active projects.</div>
        )}
      </div>

      {/* ── RECENT ACTIVITY ── */}
      <div style={S.section}>
        <div style={S.heading}>
          <span>Recent activity</span>
          <button onClick={() => onSelect({ type: 'messages' })} style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 11 }}>View all</button>
        </div>
        {recent.length > 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {recent.map(c => (
              <button key={c.conversation_id} onClick={() => onSelect({ type: 'conversation', id: c.conversation_id })} style={{
                background: 'transparent', border: 'none', borderRadius: 4,
                padding: '6px 8px', cursor: 'pointer', textAlign: 'left',
                display: 'flex', alignItems: 'center', gap: 8, transition: 'background 0.1s',
              }}
              onMouseOver={e => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-card)'; }}
              onMouseOut={e => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
              >
                <span style={{ fontSize: 12, color: 'var(--text-bright)', fontWeight: 500, minWidth: 60 }}>{c.participants.slice(0, 2).join(', ')}</span>
                <span style={{ fontSize: 12, color: 'var(--text-dim)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.preview}</span>
                <span style={{ fontSize: 10, color: 'var(--text-dim)', flexShrink: 0 }}>{relTime(c.last_message_at)}</span>
              </button>
            ))}
          </div>
        ) : (
          <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>No activity yet.</div>
        )}
      </div>

      {/* ── QUICK ACTIONS ── */}
      <div style={{ display: 'flex', gap: 8 }}>
        <button onClick={() => onSelect({ type: 'threads' })} style={{
          padding: '8px 16px', fontSize: 12, borderRadius: 6, border: '1px solid var(--border)',
          background: 'var(--bg-card)', color: 'var(--text)', cursor: 'pointer',
        }}>+ New thread</button>
        {active.length > 0 && (
          <button onClick={() => onSelect({ type: 'project', id: active[0].id })} style={{
            padding: '8px 16px', fontSize: 12, borderRadius: 6, border: '1px solid var(--border)',
            background: 'var(--bg-card)', color: 'var(--text)', cursor: 'pointer',
          }}>+ New task</button>
        )}
      </div>
    </div>
  );
}
