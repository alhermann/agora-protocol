import { useCallback, useEffect, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getStatus, getHealth, getProjects, getProjectTasks, listConsumers, getConversations, getFriends } from '../api';
import type {
  StatusResponse, HealthResponse, ProjectsResponse, ConsumersResponse,
  ConversationsResponse, FriendsResponse, ViewState,
} from '../types';

function formatUptime(secs: number): string {
  if (secs < 60) return `${Math.floor(secs)}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${h}h ${m}m`;
}

function formatRelativeTime(isoStr: string): string {
  const diff = (Date.now() - new Date(isoStr).getTime()) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function WelcomeView({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data: status } = usePolling<StatusResponse>(useCallback(() => getStatus(), []), 10000);
  const { data: health } = usePolling<HealthResponse>(useCallback(() => getHealth(), []), 10000);
  const { data: projects } = usePolling<ProjectsResponse>(useCallback(() => getProjects(), []), 10000);
  const { data: consumers } = usePolling<ConsumersResponse>(useCallback(() => listConsumers(), []), 10000);
  const { data: friends } = usePolling<FriendsResponse>(useCallback(() => getFriends(), []), 10000);
  const { data: convos } = usePolling<ConversationsResponse>(useCallback(() => getConversations(), []), 10000);

  const [taskCounts, setTaskCounts] = useState<Record<string, { total: number; done: number; todo: number; inProgress: number; blocked: number }>>({});

  const activeProjects = projects?.projects.filter(p => p.status === 'active') ?? [];

  useEffect(() => {
    activeProjects.forEach(p => {
      getProjectTasks(p.id).then(d => {
        const tasks = d.tasks ?? [];
        setTaskCounts(prev => ({
          ...prev,
          [p.id]: {
            total: tasks.length,
            done: tasks.filter(t => t.status === 'done').length,
            todo: tasks.filter(t => t.status === 'todo').length,
            inProgress: tasks.filter(t => t.status === 'in_progress').length,
            blocked: tasks.filter(t => t.status === 'blocked').length,
          }
        }));
      });
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projects]);

  const now = Date.now();
  const onlineAgents = new Set<string>();
  consumers?.consumers.forEach(c => {
    if (c.label !== 'http-default' && c.label !== status?.node_name && (now - new Date(c.last_active).getTime()) < 60000) {
      onlineAgents.add(c.label);
    }
  });

  const totalTasks = Object.values(taskCounts).reduce((s, c) => s + c.total, 0);
  const totalDone = Object.values(taskCounts).reduce((s, c) => s + c.done, 0);
  const overallPct = totalTasks > 0 ? Math.round((totalDone / totalTasks) * 100) : 0;

  const recentConvos = [...(convos?.conversations ?? [])].sort((a, b) => b.last_message_at.localeCompare(a.last_message_at)).slice(0, 5);

  return (
    <div style={{ padding: '32px', height: '100%', overflow: 'auto' }}>

      {/* Hero stats */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 16, marginBottom: 32 }}>
        <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: '20px' }}>
          <div style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 8 }}>Projects</div>
          <div style={{ fontSize: 28, fontWeight: 700, color: 'var(--text-bright)' }}>{activeProjects.length}</div>
          <div style={{ fontSize: 12, color: 'var(--text-dim)', marginTop: 4 }}>active</div>
        </div>
        <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: '20px' }}>
          <div style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 8 }}>Agents</div>
          <div style={{ fontSize: 28, fontWeight: 700, color: 'var(--green)' }}>{onlineAgents.size}</div>
          <div style={{ fontSize: 12, color: 'var(--text-dim)', marginTop: 4 }}>online / {friends?.count ?? 0} total</div>
        </div>
        <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: '20px' }}>
          <div style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 8 }}>Tasks</div>
          <div style={{ fontSize: 28, fontWeight: 700, color: 'var(--accent)' }}>{totalDone}<span style={{ fontSize: 16, color: 'var(--text-dim)' }}>/{totalTasks}</span></div>
          <div style={{ fontSize: 12, color: 'var(--text-dim)', marginTop: 4 }}>{overallPct}% complete</div>
        </div>
        <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: '20px' }}>
          <div style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 8 }}>Uptime</div>
          <div style={{ fontSize: 28, fontWeight: 700, color: 'var(--text-bright)' }}>{health ? formatUptime(health.uptime_seconds) : '--'}</div>
          <div style={{ fontSize: 12, color: status?.running ? 'var(--green)' : 'var(--red)', marginTop: 4 }}>{status?.running ? 'online' : 'offline'}</div>
        </div>
      </div>

      {/* Projects with progress */}
      <div style={{ marginBottom: 32 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)', margin: 0 }}>Projects</h3>
          <button onClick={() => onSelect({ type: 'projects' })} style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 12 }}>View all</button>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {activeProjects.map(p => {
            const tc = taskCounts[p.id];
            const pct = tc && tc.total > 0 ? Math.round((tc.done / tc.total) * 100) : 0;
            return (
              <button key={p.id} onClick={() => onSelect({ type: 'project', id: p.id })} style={{
                background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: '16px 20px',
                cursor: 'pointer', textAlign: 'left', transition: 'border-color 0.15s', display: 'flex', alignItems: 'center', gap: 16,
              }}
              onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
              onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
              >
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-bright)', marginBottom: 4 }}>{p.name}</div>
                  {p.description && <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>{p.description.slice(0, 80)}</div>}
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexShrink: 0 }}>
                  <div style={{ width: 100, height: 6, background: 'var(--bg)', borderRadius: 3, overflow: 'hidden' }}>
                    <div style={{ width: `${pct}%`, height: '100%', background: 'var(--green)', borderRadius: 3, transition: 'width 0.3s' }} />
                  </div>
                  <span style={{ fontSize: 12, color: 'var(--text-dim)', minWidth: 36 }}>{pct}%</span>
                  <span style={{ fontSize: 11, color: 'var(--text-dim)' }}>{p.agent_count} agents</span>
                </div>
              </button>
            );
          })}
          {activeProjects.length === 0 && <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>No active projects</div>}
        </div>
      </div>

      {/* Recent messages */}
      <div>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)', margin: 0 }}>Recent Activity</h3>
          <button onClick={() => onSelect({ type: 'messages' })} style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 12 }}>View all</button>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          {recentConvos.map(c => (
            <button key={c.conversation_id} onClick={() => onSelect({ type: 'conversation', id: c.conversation_id })} style={{
              background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8,
              padding: '10px 14px', cursor: 'pointer', textAlign: 'left', transition: 'border-color 0.15s',
              display: 'flex', alignItems: 'center', gap: 12,
            }}
            onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
            onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
            >
              <span style={{ fontSize: 13, color: 'var(--text-bright)', fontWeight: 500, minWidth: 100 }}>{c.participants.join(', ')}</span>
              <span style={{ fontSize: 12, color: 'var(--text-dim)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.preview}</span>
              <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>{c.message_count} msgs</span>
              <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>{formatRelativeTime(c.last_message_at)}</span>
            </button>
          ))}
          {recentConvos.length === 0 && <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>No messages yet</div>}
        </div>
      </div>
    </div>
  );
}
