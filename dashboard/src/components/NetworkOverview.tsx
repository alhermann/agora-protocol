import { useCallback, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import {
  getStatus, getPeers, getDiscoveryAgents, getDiscoveryStats, getDiscoveryProjects,
  getMarketplaceAgents, sendFriendRequest, connectToPeer,
} from '../api';
import type { MarketplaceAgent, DiscoveredAgent, DiscoveryStats, ProjectAd } from '../api';
import type { StatusResponse, PeersResponse, ViewState } from '../types';
import { TrustShield } from './TrustShield';
import { useToast } from './Toast';

function formatRelative(iso: string): string {
  const diff = (Date.now() - new Date(iso).getTime()) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function NetworkOverview({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data: status } = usePolling<StatusResponse>(useCallback(() => getStatus(), []), 10000);
  const { data: peers } = usePolling<PeersResponse>(useCallback(() => getPeers(), []), 10000);
  const { data: marketplace } = usePolling<MarketplaceAgent[]>(useCallback(() => getMarketplaceAgents(), []), 10000);
  const { data: discoveryAgents } = usePolling<{ count: number; agents: DiscoveredAgent[] }>(useCallback(() => getDiscoveryAgents(), []), 10000);
  const { data: stats } = usePolling<DiscoveryStats>(useCallback(() => getDiscoveryStats(), []), 10000);
  const { data: projectAds } = usePolling<{ count: number; projects: ProjectAd[] }>(useCallback(() => getDiscoveryProjects(), []), 10000);

  const [connectAddr, setConnectAddr] = useState('');
  const [connecting, setConnecting] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const { toast } = useToast();

  const handleConnect = async () => {
    if (!connectAddr.trim()) return;
    setConnecting(true);
    try {
      await connectToPeer(connectAddr.trim());
      toast(`Connecting to ${connectAddr}...`, 'success');
      setConnectAddr('');
    } catch {
      toast('Connection failed', 'error');
    }
    setConnecting(false);
  };

  const handleFriendRequest = async (name: string) => {
    try {
      await sendFriendRequest(name);
      toast(`Friend request sent to ${name}`, 'success');
    } catch {
      toast('Failed', 'error');
    }
  };

  // Merge marketplace + discovery agents
  const allAgents = new Map<string, { name: string; domains: string[]; tools: string[]; description?: string; availability: string; source: string }>();
  marketplace?.forEach(a => {
    if (a.agent_name !== 'mcp-monitor' && a.agent_name !== status?.node_name) {
      allAgents.set(a.agent_name, { name: a.agent_name, domains: a.domains, tools: a.tools, description: a.description, availability: a.availability, source: 'local' });
    }
  });
  discoveryAgents?.agents.forEach(a => {
    if (!allAgents.has(a.name)) {
      allAgents.set(a.name, { name: a.name, domains: a.domains, tools: a.tools, description: undefined, availability: a.availability, source: a.discovery_method });
    }
  });

  const filtered = [...allAgents.values()].filter(a => {
    if (!searchQuery) return true;
    const q = searchQuery.toLowerCase();
    return a.name.toLowerCase().includes(q)
      || a.domains.some(d => d.toLowerCase().includes(q))
      || a.tools.some(t => t.toLowerCase().includes(q))
      || (a.description?.toLowerCase().includes(q) ?? false);
  });

  return (
    <div style={{ padding: 32, height: '100%', overflow: 'auto' }}>
      <h2 style={{ fontSize: 24, fontWeight: 700, color: 'var(--text-bright)', marginBottom: 20 }}>Network</h2>

      {/* Your Identity */}
      <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: 16, marginBottom: 20 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
          <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-bright)' }}>Your Node</span>
          <span style={{ fontSize: 11, color: status?.running ? 'var(--green)' : 'var(--red)' }}>{status?.running ? 'online' : 'offline'}</span>
        </div>
        <div style={{ fontSize: 11, color: 'var(--text-dim)', fontFamily: 'var(--mono)', marginBottom: 4 }}>{status?.did}</div>
        <div style={{ fontSize: 12, color: 'var(--text)' }}>
          P2P: 0.0.0.0:7312 &middot; {peers?.count ?? 0} peer{(peers?.count ?? 0) !== 1 ? 's' : ''} connected
        </div>
      </div>

      {/* Connect to Peer */}
      <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: 16, marginBottom: 20 }}>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-bright)', marginBottom: 8 }}>Connect to Peer</div>
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            type="text" placeholder="host:port (e.g. 192.168.1.5:7312)"
            value={connectAddr} onChange={e => setConnectAddr(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && handleConnect()}
            style={{ flex: 1, padding: '8px 12px', fontSize: 13, background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 6, color: 'var(--text)', outline: 'none' }}
          />
          <button onClick={handleConnect} disabled={connecting || !connectAddr.trim()} style={{
            padding: '8px 16px', fontSize: 13, borderRadius: 6, border: 'none', cursor: 'pointer',
            background: 'var(--accent)', color: '#fff', opacity: connecting ? 0.5 : 1,
          }}>{connecting ? 'Connecting...' : 'Connect'}</button>
        </div>
        <div style={{ fontSize: 11, color: 'var(--text-dim)', marginTop: 6 }}>
          Share your address with other agents so they can connect to you
        </div>
      </div>

      {/* Connected Peers */}
      {(peers?.count ?? 0) > 0 && (
        <div style={{ marginBottom: 20 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)', marginBottom: 10 }}>Connected Peers</h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {peers?.peers.map(p => (
              <div key={p.name} style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8, padding: 12, display: 'flex', alignItems: 'center', gap: 12 }}>
                <span className="status-dot online" />
                <span style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-bright)', cursor: 'pointer' }} onClick={() => onSelect({ type: 'agent', name: p.name })}>{p.name}</span>
                <span style={{ fontSize: 10, color: 'var(--text-dim)', fontFamily: 'var(--mono)' }}>{p.did ? p.did.slice(0, 20) + '...' : ''}</span>
                {p.verified && <span style={{ fontSize: 10, color: 'var(--green)' }}>verified</span>}
                <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-dim)' }}>{p.address}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Stats */}
      <div style={{ display: 'flex', gap: 16, marginBottom: 20, fontSize: 12, color: 'var(--text-dim)' }}>
        <span>{allAgents.size} agents known</span>
        <span>{peers?.count ?? 0} peers connected</span>
        <span>{stats?.introductions ?? 0} introductions</span>
        <span>{(projectAds?.projects ?? []).length} project ads</span>
      </div>

      {/* Search */}
      <div style={{ marginBottom: 16 }}>
        <input
          type="text" placeholder="Search agents by name, domain, or capability..."
          value={searchQuery} onChange={e => setSearchQuery(e.target.value)}
          style={{ width: '100%', maxWidth: 500, padding: '10px 14px', fontSize: 13, background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8, color: 'var(--text)', outline: 'none' }}
        />
      </div>

      {/* Agent Cards */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))', gap: 12, marginBottom: 32 }}>
        {filtered.map(a => (
          <button key={a.name} onClick={() => onSelect({ type: 'agent', name: a.name })} style={{
            background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10,
            padding: 16, cursor: 'pointer', textAlign: 'left', transition: 'border-color 0.15s',
            display: 'flex', flexDirection: 'column', gap: 8,
          }}
          onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
          onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className="status-dot online" />
              <span style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-bright)' }}>{a.name}</span>
              <span style={{ marginLeft: 'auto', fontSize: 10, color: 'var(--text-dim)', textTransform: 'uppercase' }}>{a.source}</span>
            </div>
            {a.description && <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>{a.description}</div>}
            {(a.domains.length > 0 || a.tools.length > 0) && (
              <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {a.domains.map(d => <span key={d} style={{ fontSize: 10, background: 'rgba(52,208,88,0.1)', color: 'var(--green)', padding: '2px 8px', borderRadius: 10 }}>{d}</span>)}
                {a.tools.map(t => <span key={t} style={{ fontSize: 10, background: 'rgba(124,106,239,0.1)', color: 'var(--accent)', padding: '2px 8px', borderRadius: 10 }}>{t}</span>)}
              </div>
            )}
            <div style={{ display: 'flex', gap: 8, marginTop: 2 }}>
              <button onClick={e => { e.stopPropagation(); handleFriendRequest(a.name); }} style={{
                padding: '3px 10px', fontSize: 10, borderRadius: 4, border: '1px solid var(--accent)',
                background: 'none', color: 'var(--accent)', cursor: 'pointer',
              }}>Add Friend</button>
            </div>
          </button>
        ))}
      </div>

      {filtered.length === 0 && (
        <div style={{ color: 'var(--text-dim)', fontSize: 13, marginBottom: 32 }}>
          {searchQuery ? 'No agents match your search.' : 'No agents discovered yet. Connect to a peer above to start discovering.'}
        </div>
      )}

      {/* Project Ads */}
      {(projectAds?.projects ?? []).length > 0 && (
        <>
          <h3 style={{ fontSize: 18, fontWeight: 600, color: 'var(--text-bright)', marginBottom: 12 }}>Projects Looking for Contributors</h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {(projectAds?.projects ?? []).map(p => (
              <div key={p.project_id} style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 10, padding: 16 }}>
                <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-bright)', marginBottom: 4 }}>{p.project_name}</div>
                {p.description && <div style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 8 }}>{p.description}</div>}
                <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {p.open_roles.map((r, i) => (
                    <div key={i} style={{ background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 8, padding: '6px 10px' }}>
                      <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--accent)' }}>{r.role}</div>
                      {r.desired_domains.length > 0 && <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>Needs: {r.desired_domains.join(', ')}</div>}
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
