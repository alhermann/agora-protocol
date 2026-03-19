import { useCallback } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getStatus, getFriends, getPeers, getProjects, listConsumers } from '../api';
import { TrustShield } from './TrustShield';
import type { StatusResponse, FriendsResponse, PeersResponse, ProjectsResponse, ConsumersResponse, ViewState } from '../types';

export function AgentsOverview({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data: status } = usePolling<StatusResponse>(useCallback(() => getStatus(), []), 10000);
  const { data: friends } = usePolling<FriendsResponse>(useCallback(() => getFriends(), []), 10000);
  const { data: peers } = usePolling<PeersResponse>(useCallback(() => getPeers(), []), 10000);
  const { data: projects } = usePolling<ProjectsResponse>(useCallback(() => getProjects(), []), 10000);
  const { data: consumers } = usePolling<ConsumersResponse>(useCallback(() => listConsumers(), []), 10000);

  const now = Date.now();
  const activeConsumers = new Set<string>();
  consumers?.consumers.forEach(c => {
    if (c.label !== 'http-default' && (now - new Date(c.last_active).getTime()) < 60000) {
      activeConsumers.add(c.label);
    }
  });

  const peerMap = new Map<string, boolean>();
  peers?.peers.forEach(p => peerMap.set(p.name, true));

  // Build agent list with roles from projects
  const agentRoles = new Map<string, string[]>();
  projects?.projects.filter(p => p.status === 'active').forEach(p => {
    (p.agent_names ?? []).forEach(name => {
      if (!agentRoles.has(name)) agentRoles.set(name, []);
    });
  });

  const myName = status?.node_name ?? '';
  const allNames = new Set<string>();
  friends?.friends.forEach(f => { if (f.name !== myName) allNames.add(f.name); });
  peers?.peers.forEach(p => { if (p.name !== myName) allNames.add(p.name); });
  agentRoles.forEach((_, name) => { if (name !== myName) allNames.add(name); });

  const agents = [...allNames].map(name => {
    const friend = friends?.friends.find(f => f.name === name);
    const online = peerMap.has(name) || activeConsumers.has(name);
    return { name, online, friend, trust: friend?.trust_level ?? 0, trustName: friend?.trust_name ?? 'Unknown' };
  }).sort((a, b) => {
    if (a.online !== b.online) return a.online ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  return (
    <div style={{ padding: 32, height: '100%', overflow: 'auto' }}>
      <h2 style={{ fontSize: 24, fontWeight: 700, color: 'var(--text-bright)', marginBottom: 24 }}>Agents</h2>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 16 }}>
        {agents.map(a => {
          // Find which projects this agent is in
          const inProjects = projects?.projects.filter(p => p.status === 'active' && p.agent_names?.includes(a.name)) ?? [];

          return (
            <button
              key={a.name}
              onClick={() => onSelect({ type: 'agent', name: a.name })}
              style={{
                background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10,
                padding: 20, cursor: 'pointer', textAlign: 'left', transition: 'all 0.15s',
                display: 'flex', flexDirection: 'column', gap: 10,
              }}
              onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
              onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <span className={`status-dot ${a.online ? 'online' : 'offline'}`} />
                <span style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>{a.name}</span>
                <TrustShield level={a.trust} size={18} />
                <span style={{ marginLeft: 'auto', fontSize: 11, color: a.online ? 'var(--green)' : 'var(--text-dim)' }}>
                  {a.online ? 'online' : 'offline'}
                </span>
              </div>

              <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>
                Trust: {a.trustName} ({a.trust})
              </div>

              {inProjects.length > 0 && (
                <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                  {inProjects.map(p => (
                    <span key={p.id} style={{
                      fontSize: 10, background: 'rgba(88,166,255,0.1)', color: 'var(--accent)',
                      padding: '2px 8px', borderRadius: 10,
                    }}>{p.name}</span>
                  ))}
                </div>
              )}
            </button>
          );
        })}
      </div>

      {agents.length === 0 && (
        <div style={{ color: 'var(--text-dim)', fontSize: 14 }}>No agents yet. Add friends or connect to peers.</div>
      )}
    </div>
  );
}
