import { useCallback, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getPeers, getFriends, getConversations, getProjects, listConsumers, updateFriend, removeFriend, sendFriendRequest, disconnectPeer, getMarketplaceAgents, getDiscoveryAgent } from '../api';
import type { MarketplaceAgent, DiscoveredAgent } from '../api';
import { TrustShield } from './TrustShield';
import type {
  PeersResponse, FriendsResponse, ConversationsResponse, ProjectsResponse, ConsumersResponse,
  FriendEntry, PeerEntry, ViewState,
} from '../types';

function truncateDid(did: string): string {
  if (did.length <= 20) return did;
  return did.slice(0, 14) + '...';
}

export function AgentDetail({
  name,
  onSelect,
}: {
  name: string;
  onSelect: (v: ViewState) => void;
}) {
  const fetchPeers = useCallback(() => getPeers(), []);
  const fetchFriends = useCallback(() => getFriends(), []);
  const fetchConvos = useCallback(() => getConversations(), []);
  const fetchProjects = useCallback(() => getProjects(), []);

  const { data: peers } = usePolling<PeersResponse>(fetchPeers, 10000);
  const { data: friends, refresh: refreshFriends } = usePolling<FriendsResponse>(fetchFriends, 10000);
  const { data: convos } = usePolling<ConversationsResponse>(fetchConvos, 10000);
  const { data: projectsData } = usePolling<ProjectsResponse>(fetchProjects, 10000);
  const fetchConsumers = useCallback(() => listConsumers(), []);
  const { data: consumersData } = usePolling<ConsumersResponse>(fetchConsumers, 10000);
  const { data: marketplace } = usePolling<MarketplaceAgent[]>(useCallback(() => getMarketplaceAgents(), []), 10000);

  const agentCaps = marketplace?.find(a => a.agent_name === name);

  const [removing, setRemoving] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [adding, setAdding] = useState(false);
  const [didCopied, setDidCopied] = useState(false);

  const peer: PeerEntry | undefined = peers?.peers.find((p) => p.name === name);
  const friend: FriendEntry | undefined = friends?.friends.find((f) => f.name === name);
  const consumerOnline = consumersData?.consumers.some(c => {
    if (c.label !== name) return false;
    const age = (Date.now() - new Date(c.last_active).getTime()) / 1000;
    return age < 60;
  }) ?? false;
  const online = !!peer || consumerOnline;
  const isUnknown = !friend;

  const trustLabels = ['Unknown', 'Acquaintance', 'Friend', 'Trusted', 'Full Trust'];

  const handleTrustChange = async (newLevel: number) => {
    try {
      await updateFriend(name, { trust_level: newLevel });
      refreshFriends();
    } catch { /* ignore */ }
  };

  const handleMuteToggle = async () => {
    if (!friend) return;
    try {
      await updateFriend(name, { muted: !friend.muted });
      refreshFriends();
    } catch { /* ignore */ }
  };

  const handleRemove = async () => {
    if (!confirmRemove) {
      setConfirmRemove(true);
      return;
    }
    setRemoving(true);
    try {
      await removeFriend(name);
      onSelect({ type: 'welcome' });
    } catch { /* ignore */ }
    setRemoving(false);
    setConfirmRemove(false);
  };

  const handleDisconnect = async () => {
    setDisconnecting(true);
    try {
      await disconnectPeer(name);
    } catch { /* ignore */ }
    setDisconnecting(false);
  };

  const handleAddFriend = async () => {
    setAdding(true);
    try {
      await sendFriendRequest(name, 2);
      refreshFriends();
    } catch { /* ignore */ }
    setAdding(false);
  };

  const handleCopyDid = (did: string) => {
    navigator.clipboard.writeText(did).then(() => {
      setDidCopied(true);
      setTimeout(() => setDidCopied(false), 2000);
    });
  };

  // Conversations involving this agent
  const relatedConvos = convos?.conversations.filter(
    (c) => c.participants.includes(name)
  ) ?? [];

  // Projects this agent is part of
  const agentProjects = projectsData?.projects.filter(
    (p) => p.status === 'active' && p.agent_names?.includes(name)
  ) ?? [];

  // Resolve the DID to display (prefer friend's pinned DID, fall back to peer's)
  const displayDid = friend?.did ?? peer?.did;

  return (
    <div className="agent-detail">
      {/* Header: back button + dot + name */}
      <div className="detail-header">
        <button className="back-btn" onClick={() => onSelect({ type: 'welcome' })} title="Back to overview">{'\u2190'}</button>
        <span className={`status-dot ${online ? 'online' : 'offline'}`} />
        <h2 className="detail-name">{name}</h2>
        <TrustShield level={friend?.trust_level ?? 0} size={22} />
      </div>

      {/* Unknown agent banner */}
      {isUnknown && online && (
        <div className="detail-unknown-banner">
          <p className="detail-unknown-text">
            This agent isn't in your contacts.
          </p>
          <div className="detail-unknown-controls">
            <button
              className="btn-sm btn-primary"
              onClick={handleAddFriend}
              disabled={adding}
            >
              {adding ? 'Sending...' : 'Add as Friend'}
            </button>
            <button
              className="btn-sm btn-danger"
              onClick={handleDisconnect}
              disabled={disconnecting}
            >
              {disconnecting ? 'Disconnecting...' : 'Disconnect'}
            </button>
          </div>
        </div>
      )}

      <div className="detail-info">
        {/* Trust dropdown (friends only) */}
        {friend && (
          <div className="detail-row">
            <span className="detail-label">Trust</span>
            <select
              className="select-sm"
              value={friend.trust_level}
              onChange={(e) => handleTrustChange(Number(e.target.value))}
            >
              {trustLabels.map((label, i) => (
                <option key={i} value={i}>{label}</option>
              ))}
            </select>
          </div>
        )}

        {/* Identity (collapsed DID with copy) */}
        {displayDid && (
          <div className="detail-row">
            <span className="detail-label">Identity</span>
            <span className="detail-value dim">
              {truncateDid(displayDid)}
              <button
                className="btn-copy"
                onClick={() => handleCopyDid(displayDid)}
                title="Copy full DID"
              >
                {didCopied ? 'Copied!' : 'Copy'}
              </button>
            </span>
          </div>
        )}

        {/* Notifications toggle (friends only) */}
        {friend && (
          <div className="detail-row">
            <span className="detail-label">Notifications</span>
            <button
              className={`toggle-btn ${friend.muted ? 'off' : 'on'}`}
              onClick={handleMuteToggle}
            >
              <span className="toggle-knob" />
            </button>
          </div>
        )}
      </div>

      {/* Conversations with this agent */}
      <div className="detail-section">
        <h3 className="detail-section-heading">Conversations</h3>
        {relatedConvos.length === 0 ? (
          <div className="empty">No conversations with {name} yet.</div>
        ) : (
          <div className="detail-convo-list">
            {relatedConvos.map((c) => (
              <button
                key={c.conversation_id}
                className="detail-convo-item"
                onClick={() => onSelect({ type: 'conversation', id: c.conversation_id })}
              >
                <span className="detail-convo-preview">{c.preview}</span>
                <span className="detail-convo-meta">{c.message_count} messages</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Projects this agent is involved in */}
      <div className="detail-section">
        <h3 className="detail-section-heading">Projects</h3>
        {agentProjects.length === 0 ? (
          <div className="empty">Not in any active projects.</div>
        ) : (
          <div className="detail-convo-list">
            {agentProjects.map((p) => (
              <button
                key={p.id}
                className="detail-convo-item"
                onClick={() => onSelect({ type: 'project', id: p.id })}
              >
                <span className="detail-convo-preview">{p.name}</span>
                <span className="detail-convo-meta">{p.agent_count} agents</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Capabilities from marketplace */}
      {agentCaps && (agentCaps.domains.length > 0 || agentCaps.tools.length > 0) && (
        <div className="detail-section">
          <h3 className="detail-section-heading">Capabilities</h3>
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            {agentCaps.domains.map(d => (
              <span key={d} style={{
                fontSize: 12, background: 'rgba(52,208,88,0.1)', color: 'var(--green)',
                padding: '4px 10px', borderRadius: 10,
              }}>{d}</span>
            ))}
            {agentCaps.tools.map(t => (
              <span key={t} style={{
                fontSize: 12, background: 'rgba(124,106,239,0.1)', color: 'var(--accent)',
                padding: '4px 10px', borderRadius: 10,
              }}>{t}</span>
            ))}
          </div>
          <div style={{ fontSize: 12, color: 'var(--text-dim)', marginTop: 8 }}>
            Availability: {agentCaps.availability}
          </div>
        </div>
      )}

      {/* Mutual trust indicator */}
      {friend && (
        <div className="detail-section">
          <h3 className="detail-section-heading">Trust Relationship</h3>
          <div style={{ fontSize: 13, color: 'var(--text)' }}>
            <div>You trust {name}: <strong>{trustLabels[friend.trust_level]}</strong> ({friend.trust_level})</div>
            {friend.their_trust !== undefined && friend.their_trust !== null && (
              <div>{name} trusts you: <strong>{trustLabels[friend.their_trust] || 'Unknown'}</strong> ({friend.their_trust})</div>
            )}
          </div>
        </div>
      )}

      {/* Remove friend */}
      {friend && friend.trust_level > 0 && (
        <div className="detail-danger">
          {confirmRemove ? (
            <div className="confirm-dialog">
              <p className="confirm-text">
                Remove {name} and disconnect? This will also unpin their DID.
              </p>
              <div className="confirm-actions">
                <button
                  className="btn-sm btn-danger"
                  onClick={handleRemove}
                  disabled={removing}
                >
                  {removing ? 'Removing...' : 'Confirm Remove'}
                </button>
                <button
                  className="btn-sm btn-cancel"
                  onClick={() => setConfirmRemove(false)}
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              className="btn-sm btn-danger"
              onClick={handleRemove}
            >
              Remove friend
            </button>
          )}
        </div>
      )}
    </div>
  );
}
