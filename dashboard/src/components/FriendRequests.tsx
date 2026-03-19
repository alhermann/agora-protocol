import { useCallback, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getFriendRequests, acceptFriendRequest, rejectFriendRequest } from '../api';
import { TrustShield } from './TrustShield';
import type { FriendRequestsResponse, FriendRequestEntry } from '../types';

export function FriendRequests({ onBack }: { onBack?: () => void }) {
  const fetchRequests = useCallback(() => getFriendRequests(), []);
  const { data, refresh } = usePolling<FriendRequestsResponse>(fetchRequests, 10000);

  const [acceptingId, setAcceptingId] = useState<string | null>(null);
  const [rejectingId, setRejectingId] = useState<string | null>(null);
  const [trustLevels, setTrustLevels] = useState<Record<string, number>>({});

  const trustLabels = ['Unknown', 'Acquaintance', 'Friend', 'Trusted', 'Inner Circle'];

  const inbound = data?.requests.filter(
    (r) => r.direction === 'inbound' && r.status === 'pending'
  ) ?? [];

  const outbound = data?.requests.filter(
    (r) => r.direction === 'outbound' && r.status === 'pending'
  ) ?? [];

  const resolved = data?.requests.filter(
    (r) => r.status !== 'pending'
  ) ?? [];

  const handleAccept = async (req: FriendRequestEntry) => {
    setAcceptingId(req.id);
    try {
      const trust = trustLevels[req.id] ?? 2;
      await acceptFriendRequest(req.id, trust);
      refresh();
    } catch { /* ignore */ }
    setAcceptingId(null);
  };

  const handleReject = async (req: FriendRequestEntry) => {
    setRejectingId(req.id);
    try {
      await rejectFriendRequest(req.id);
      refresh();
    } catch { /* ignore */ }
    setRejectingId(null);
  };

  return (
    <div className="friend-requests">
      <div className="detail-header">
        {onBack && <button className="back-btn" onClick={onBack} title="Back to overview">{'\u2190'}</button>}
        <h2 className="friend-requests-title">Friend Requests</h2>
      </div>

      {/* Inbound pending */}
      <section className="friend-requests-section">
        <h3 className="friend-requests-heading">
          Incoming
          {inbound.length > 0 && (
            <span className="friend-requests-badge">{inbound.length}</span>
          )}
        </h3>
        {inbound.length === 0 ? (
          <div className="empty">No pending incoming requests.</div>
        ) : (
          <div className="friend-requests-list">
            {inbound.map((req) => (
              <div key={req.id} className="friend-request-card">
                <div className="friend-request-header">
                  <TrustShield level={req.offered_trust} size={20} />
                  <span className="friend-request-name">{req.peer_name}</span>
                  <span className="friend-request-trust">
                    offers trust {req.offered_trust} ({req.offered_trust_name})
                  </span>
                </div>
                {req.message && (
                  <div className="friend-request-message">"{req.message}"</div>
                )}
                <div className="friend-request-meta">
                  {new Date(req.created_at).toLocaleString()}
                </div>
                <div className="friend-request-actions">
                  <label className="friend-request-trust-label">Your trust:</label>
                  <select
                    className="select-sm"
                    value={trustLevels[req.id] ?? 2}
                    onChange={(e) =>
                      setTrustLevels((prev) => ({
                        ...prev,
                        [req.id]: Number(e.target.value),
                      }))
                    }
                  >
                    {trustLabels.map((label, i) => (
                      <option key={i} value={i}>
                        {i} — {label}
                      </option>
                    ))}
                  </select>
                  <button
                    className="btn-sm btn-primary"
                    onClick={() => handleAccept(req)}
                    disabled={acceptingId === req.id}
                  >
                    {acceptingId === req.id ? 'Accepting...' : 'Accept'}
                  </button>
                  <button
                    className="btn-sm btn-danger"
                    onClick={() => handleReject(req)}
                    disabled={rejectingId === req.id}
                  >
                    {rejectingId === req.id ? 'Rejecting...' : 'Reject'}
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* Outbound pending */}
      <section className="friend-requests-section">
        <h3 className="friend-requests-heading">Outgoing</h3>
        {outbound.length === 0 ? (
          <div className="empty">No pending outgoing requests.</div>
        ) : (
          <div className="friend-requests-list">
            {outbound.map((req) => (
              <div key={req.id} className="friend-request-card outbound">
                <div className="friend-request-header">
                  <TrustShield level={req.offered_trust} size={20} />
                  <span className="friend-request-name">{req.peer_name}</span>
                  <span className="friend-request-trust">
                    offering trust {req.offered_trust} ({req.offered_trust_name})
                  </span>
                </div>
                {req.message && (
                  <div className="friend-request-message">"{req.message}"</div>
                )}
                <div className="friend-request-meta">
                  Sent {new Date(req.created_at).toLocaleString()} — waiting for response
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* Resolved history */}
      {resolved.length > 0 && (
        <section className="friend-requests-section">
          <h3 className="friend-requests-heading">History</h3>
          <div className="friend-requests-list">
            {resolved.slice(0, 20).map((req) => (
              <div
                key={req.id}
                className={`friend-request-card resolved ${req.status}`}
              >
                <div className="friend-request-header">
                  <span className="friend-request-name">{req.peer_name}</span>
                  <span className={`friend-request-status ${req.status}`}>
                    {req.status}
                  </span>
                  <span className="friend-request-direction">({req.direction})</span>
                </div>
                <div className="friend-request-meta">
                  {req.resolved_at
                    ? new Date(req.resolved_at).toLocaleString()
                    : new Date(req.created_at).toLocaleString()}
                </div>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
