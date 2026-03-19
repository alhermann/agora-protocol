import { useCallback } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getProjects } from '../api';
import type { ProjectsResponse, ViewState } from '../types';

export function ProjectsOverview({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const fetchProjects = useCallback(() => getProjects(), []);
  const { data: projectsData } = usePolling<ProjectsResponse>(fetchProjects, 10000);

  const projects = projectsData?.projects ?? [];
  const active = projects.filter(p => p.status === 'active');
  const archived = projects.filter(p => p.status === 'archived');

  return (
    <div style={{ padding: '32px', height: '100%', overflow: 'auto' }}>
      <h2 style={{ fontSize: 24, fontWeight: 700, color: 'var(--text-bright)', marginBottom: 24 }}>Projects</h2>

      {active.length === 0 && (
        <div style={{ color: 'var(--text-dim)', fontSize: 14 }}>No active projects. Create one from the sidebar.</div>
      )}

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: 16 }}>
        {active.map(p => (
          <button
            key={p.id}
            onClick={() => onSelect({ type: 'project', id: p.id })}
            style={{
              background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10,
              padding: 20, cursor: 'pointer', textAlign: 'left', transition: 'all 0.15s',
              display: 'flex', flexDirection: 'column', gap: 10,
            }}
            onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
            onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className="status-dot online" />
              <span style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>{p.name}</span>
            </div>

            {p.description && (
              <div style={{ fontSize: 13, color: 'var(--text-dim)', lineHeight: 1.4 }}>
                {p.description.length > 100 ? p.description.slice(0, 100) + '...' : p.description}
              </div>
            )}

            <div style={{ display: 'flex', gap: 16, fontSize: 12, color: 'var(--text-dim)' }}>
              <span>{p.agent_count} agents</span>
              {p.agent_names && <span>{p.agent_names.join(', ')}</span>}
            </div>

            {p.repo && (
              <div style={{ fontSize: 11, color: 'var(--accent)' }}>
                {p.repo.replace('https://github.com/', '')}
              </div>
            )}
          </button>
        ))}
      </div>

      {archived.length > 0 && (
        <>
          <h3 style={{ fontSize: 14, color: 'var(--text-dim)', marginTop: 32, marginBottom: 12, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
            Archived ({archived.length})
          </h3>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: 12 }}>
            {archived.map(p => (
              <button
                key={p.id}
                onClick={() => onSelect({ type: 'project', id: p.id })}
                style={{
                  background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 8,
                  padding: 14, cursor: 'pointer', textAlign: 'left', opacity: 0.6,
                }}
              >
                <span style={{ fontSize: 14, color: 'var(--text-dim)' }}>{p.name}</span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
