import { useCallback, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getStatus, getFriends, getPeers, getProjects, listConsumers, getMarketplaceAgents, searchMarketplace } from '../api';
import type { MarketplaceAgent } from '../api';
import { TrustShield } from './TrustShield';
import type { StatusResponse, FriendsResponse, PeersResponse, ProjectsResponse, ConsumersResponse, ViewState } from '../types';

export function AgentsOverview({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data: status } = usePolling<StatusResponse>(useCallback(() => getStatus(), []), 10000);
  const { data: friends } = usePolling<FriendsResponse>(useCallback(() => getFriends(), []), 10000);
  const { data: peers } = usePolling<PeersResponse>(useCallback(() => getPeers(), []), 10000);
  const { data: projects } = usePolling<ProjectsResponse>(useCallback(() => getProjects(), []), 10000);
  const { data: consumers } = usePolling<ConsumersResponse>(useCallback(() => listConsumers(), []), 10000);
  const { data: marketplace } = usePolling<MarketplaceAgent[]>(useCallback(() => getMarketplaceAgents(), []), 10000);

  const [searchQuery, setSearchQuery] = useState('');

  const now = Date.now();
  const activeConsumers = new Set<string>();
  consumers?.consumers.forEach(c => {
    if (c.label !== 'http-default' && (now - new Date(c.last_active).getTime()) < 60000) {
      activeConsumers.add(c.label);
    }
  });

  const peerMap = new Map<string, boolean>();
  peers?.peers.forEach(p => peerMap.set(p.name, true));

  // Build marketplace capability map
  const capMap = new Map<string, MarketplaceAgent>();
  marketplace?.forEach(a => capMap.set(a.agent_name, a));

  const myName = status?.node_name ?? '';
  const allNames = new Set<string>();
  friends?.friends.forEach(f => { if (f.name !== myName) allNames.add(f.name); });
  peers?.peers.forEach(p => { if (p.name !== myName) allNames.add(p.name); });
  projects?.projects.filter(p => p.status === 'active').forEach(p => {
    (p.agent_names ?? []).forEach(name => { if (name !== myName) allNames.add(name); });
  });
  // Also include marketplace agents not in friend list
  marketplace?.forEach(a => { if (a.agent_name !== myName) allNames.add(a.agent_name); });

  const agents = [...allNames].map(name => {
    const friend = friends?.friends.find(f => f.name === name);
    const online = peerMap.has(name) || activeConsumers.has(name);
    const caps = capMap.get(name);
    return { name, online, friend, trust: friend?.trust_level ?? 0, trustName: friend?.trust_name ?? 'Unknown', caps };
  }).filter(a => {
    if (!searchQuery) return true;
    const q = searchQuery.toLowerCase();
    if (a.name.toLowerCase().includes(q)) return true;
    if (a.caps?.domains.some(d => d.toLowerCase().includes(q))) return true;
    if (a.caps?.tools.some(t => t.toLowerCase().includes(q))) return true;
    return false;
  }).sort((a, b) => {
    if (a.online !== b.online) return a.online ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  return (
    <div style={{ padding: 32, height: '100%', overflow: 'auto' }}>
      <h2 style={{ fontSize: 24, fontWeight: 700, color: 'var(--text-bright)', marginBottom: 16 }}>Agents</h2>

      {/* Search */}
      <div style={{ marginBottom: 20 }}>
        <input
          type="text"
          placeholder="Search by name, domain, or capability..."
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
          style={{
            width: '100%', maxWidth: 400, padding: '8px 12px', fontSize: 13,
            background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8,
            color: 'var(--text)', outline: 'none',
          }}
        />
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))', gap: 16 }}>
        {agents.map(a => {
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
              {/* Header: name + status */}
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <span className={`status-dot ${a.online ? 'online' : 'offline'}`} />
                <span style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>{a.name}</span>
                <TrustShield level={a.trust} size={18} />
                <span style={{ marginLeft: 'auto', fontSize: 11, color: a.online ? 'var(--green)' : 'var(--text-dim)' }}>
                  {a.online ? 'online' : 'offline'}
                </span>
              </div>

              {/* Capabilities */}
              {a.caps && (a.caps.domains.length > 0 || a.caps.tools.length > 0) && (
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {a.caps.domains.map(d => (
                    <span key={d} style={{
                      fontSize: 10, background: 'rgba(52,208,88,0.1)', color: 'var(--green)',
                      padding: '2px 8px', borderRadius: 10,
                    }}>{d}</span>
                  ))}
                  {a.caps.tools.map(t => (
                    <span key={t} style={{
                      fontSize: 10, background: 'rgba(124,106,239,0.1)', color: 'var(--accent)',
                      padding: '2px 8px', borderRadius: 10,
                    }}>{t}</span>
                  ))}
                </div>
              )}

              {/* Projects */}
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

              {/* Trust */}
              <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                Trust: {a.trustName} ({a.trust})
              </div>
            </button>
          );
        })}
      </div>

      {agents.length === 0 && (
        <div style={{ color: 'var(--text-dim)', fontSize: 14 }}>
          {searchQuery ? 'No agents match your search.' : 'No agents yet. Add friends or connect to peers.'}
        </div>
      )}
    </div>
  );
}
