import { useCallback, useState, useEffect } from 'react';
import { usePolling } from '../hooks/usePolling';
import {
  getStatus, getHealth, getPeers, getFriends, listConsumers,
  getConversations, getProjects, getProjectRooms,
} from '../api';
import type {
  StatusResponse, HealthResponse, PeersResponse, ConsumersResponse,
  FriendsResponse, ConversationsResponse,
  FriendEntry, PeerEntry, ConversationSummary, ViewState, ProjectsResponse,
  ProjectRoomsResponse, ProjectEntry,
} from '../types';

function formatRelativeTime(isoStr: string): string {
  const diff = (Date.now() - new Date(isoStr).getTime()) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

interface MergedAgent {
  name: string;
  online: boolean;
  friend: FriendEntry | null;
  peer: PeerEntry | null;
  role: string | null;
}

export function Sidebar({
  selectedView,
  onSelect,
}: {
  selectedView: ViewState;
  onSelect: (v: ViewState) => void;
}) {
  const fetchStatus = useCallback(() => getStatus(), []);
  const fetchHealth = useCallback(() => getHealth(), []);
  const fetchPeers = useCallback(() => getPeers(), []);
  const fetchFriends = useCallback(() => getFriends(), []);
  const fetchConvos = useCallback(() => getConversations(), []);
  const fetchProjects = useCallback(() => getProjects(), []);

  const { data: status, error: statusErr } = usePolling<StatusResponse>(fetchStatus, 10000);
  const { data: _health } = usePolling<HealthResponse>(fetchHealth, 10000);
  const { data: peers } = usePolling<PeersResponse>(fetchPeers, 10000);
  const { data: friends } = usePolling<FriendsResponse>(fetchFriends, 10000);
  const { data: convos } = usePolling<ConversationsResponse>(fetchConvos, 10000);
  const { data: projectsData } = usePolling<ProjectsResponse>(fetchProjects, 10000);
  const fetchConsumers = useCallback(() => listConsumers(), []);
  const { data: consumersData } = usePolling<ConsumersResponse>(fetchConsumers, 10000);

  // Suppress unused var warning
  void _health;

  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [projectRooms, setProjectRooms] = useState<Record<string, ProjectRoomsResponse>>({});

  const online = !statusErr && status?.running;

  // Fetch rooms for active projects
  const activeProjects = projectsData?.projects.filter(p => p.status === 'active') ?? [];

  useEffect(() => {
    activeProjects.forEach((p) => {
      getProjectRooms(p.id).then((rooms) => {
        setProjectRooms((prev) => ({ ...prev, [p.id]: rooms }));
      });
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectsData]);

  useEffect(() => {
    if (activeProjects.length === 0) return;
    const interval = setInterval(() => {
      activeProjects.forEach((p) => {
        getProjectRooms(p.id).then((rooms) => {
          setProjectRooms((prev) => ({ ...prev, [p.id]: rooms }));
        });
      });
    }, 10000);
    return () => clearInterval(interval);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectsData]);

  // Build set of conversation IDs that belong to project rooms
  const roomConversationIds = new Set<string>();
  Object.values(projectRooms).forEach((pr) => {
    pr.rooms.forEach((room) => {
      roomConversationIds.add(room.conversation_id);
    });
  });

  // Direct messages = conversations not belonging to any room
  const directMessages = convos?.conversations.filter(
    (c) => !roomConversationIds.has(c.conversation_id)
  ) ?? [];

  // Find conversation message count for a room's conversation_id
  const getConvoMsgCount = (conversationId: string): number | null => {
    const convo = convos?.conversations.find(c => c.conversation_id === conversationId);
    return convo?.message_count ?? null;
  };

  // Build agent role map from projects
  const agentRoleMap = new Map<string, string>();
  projectsData?.projects
    .filter((p) => p.status === 'active')
    .forEach((p) => {
      // We'll get roles from project detail later; for now use project agent_names
      if (p.agent_names) {
        p.agent_names.forEach((n) => {
          if (!agentRoleMap.has(n)) agentRoleMap.set(n, '');
        });
      }
    });

  // Merge friends + peers + consumers into unified list
  const peerMap = new Map<string, PeerEntry>();
  peers?.peers.forEach((p) => peerMap.set(p.name, p));

  const activeConsumers = new Set<string>();
  const now = Date.now();
  consumersData?.consumers.forEach((c) => {
    if (c.label !== 'http-default' && c.label !== status?.node_name) {
      const lastActive = new Date(c.last_active).getTime();
      if (now - lastActive < 60000) {
        activeConsumers.add(c.label);
      }
    }
  });

  const agents: MergedAgent[] = [];
  const seen = new Set<string>();

  friends?.friends.forEach((f) => {
    if (f.name === status?.node_name) return;
    seen.add(f.name);
    agents.push({
      name: f.name,
      online: peerMap.has(f.name) || activeConsumers.has(f.name),
      friend: f,
      peer: peerMap.get(f.name) ?? null,
      role: agentRoleMap.get(f.name) ?? null,
    });
  });
  peers?.peers.forEach((p) => {
    if (!seen.has(p.name)) {
      seen.add(p.name);
      agents.push({ name: p.name, online: true, friend: null, peer: p, role: agentRoleMap.get(p.name) ?? null });
    }
  });

  projectsData?.projects
    .filter((p) => p.status === 'active' && p.agent_names)
    .forEach((p) => {
      p.agent_names!.forEach((agentName) => {
        if (!seen.has(agentName) && agentName !== status?.node_name) {
          seen.add(agentName);
          agents.push({ name: agentName, online: false, friend: null, peer: null, role: agentRoleMap.get(agentName) ?? null });
        }
      });
    });

  agents.sort((a, b) => {
    if (a.online !== b.online) return a.online ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  const toggleProject = (projectId: string) => {
    setExpandedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(projectId)) {
        next.delete(projectId);
      } else {
        next.add(projectId);
      }
      return next;
    });
  };

  const isRoomSelected = (conversationId: string) =>
    selectedView.type === 'conversation' && selectedView.id === conversationId;

  const isProjectSelected = (projectId: string) =>
    selectedView.type === 'project' && selectedView.id === projectId;

  return (
    <aside className="sidebar">
      {/* Brand + Agent Identity */}
      <div className="sidebar-identity">
        <div className="sidebar-brand">AGORA</div>
        <div className="sidebar-self">
          <span className={`status-dot-sm ${online ? 'online' : 'offline'}`} />
          <span className="sidebar-self-name">{status?.node_name ?? '...'}</span>
          <span className="sidebar-self-status">{online ? 'online' : 'offline'}</span>
        </div>
      </div>

      <div className="sidebar-divider" />

      {/* Projects with nested rooms */}
      <section className="sidebar-section">
        <h3 className="sidebar-heading">
          <button
            onClick={() => onSelect({ type: 'projects' })}
            style={{ background: 'none', border: 'none', color: 'inherit', cursor: 'pointer', font: 'inherit', padding: 0, textTransform: 'inherit', letterSpacing: 'inherit', fontWeight: 'inherit' }}
          >
            Projects
          </button>
        </h3>

        {activeProjects.length > 0 ? (
          activeProjects.map((p: ProjectEntry) => {
            const rooms = projectRooms[p.id]?.rooms ?? [];
            const isExpanded = expandedProjects.has(p.id);
            const hasSelectedRoom = rooms.some(r => isRoomSelected(r.conversation_id));
            const expanded = isExpanded || isProjectSelected(p.id) || hasSelectedRoom;

            return (
              <div key={p.id} className="sidebar-project-group">
                <button
                  className={`sidebar-project-header ${isProjectSelected(p.id) ? 'selected' : ''}`}
                  onClick={() => {
                    toggleProject(p.id);
                    onSelect({ type: 'project', id: p.id });
                  }}
                >
                  <span className={`sidebar-project-expand ${expanded ? 'expanded' : ''}`}>
                    {'\u25B8'}
                  </span>
                  <span className="sidebar-project-name">{p.name}</span>
                </button>

                {expanded && (
                  <div className="sidebar-project-rooms">
                    {rooms.map((room) => {
                      const msgCount = getConvoMsgCount(room.conversation_id);
                      const selected = isRoomSelected(room.conversation_id);
                      return (
                        <button
                          key={room.room_id}
                          className={`sidebar-room ${selected ? 'selected' : ''}`}
                          onClick={() => onSelect({ type: 'conversation', id: room.conversation_id })}
                        >
                          <span className="sidebar-room-name">{room.name}</span>
                          {msgCount != null && msgCount > 0 && (
                            <span className="sidebar-room-badge">{msgCount}</span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })
        ) : (
          <div className="sidebar-empty">No projects yet</div>
        )}
      </section>

      <div className="sidebar-divider" />

      {/* Agents */}
      <section className="sidebar-section">
        <h3 className="sidebar-heading">
          <button
            onClick={() => onSelect({ type: 'agents' })}
            style={{ background: 'none', border: 'none', color: 'inherit', cursor: 'pointer', font: 'inherit', padding: 0, textTransform: 'inherit', letterSpacing: 'inherit', fontWeight: 'inherit' }}
          >
            Agents
          </button>
        </h3>

        {agents.length === 0 && <div className="sidebar-empty">No agents yet</div>}

        {agents.map((a) => {
          const isSelected = selectedView.type === 'agent' && selectedView.name === a.name;
          return (
            <button
              key={a.name}
              className={`sidebar-agent-item ${isSelected ? 'selected' : ''}`}
              onClick={() => onSelect({ type: 'agent', name: a.name })}
            >
              <span className={`status-dot-sm ${a.online ? 'online' : 'offline'}`} />
              <span className="sidebar-agent-item-name">{a.name}</span>
              {a.role && (
                <span className="sidebar-agent-role-tag">{a.role}</span>
              )}
            </button>
          );
        })}
      </section>

      {/* Direct Messages — only shown if they exist */}
      {directMessages.length > 0 && (
        <>
          <div className="sidebar-divider" />
          <section className="sidebar-section sidebar-dm-section">
            <h3 className="sidebar-heading">
              <button
                onClick={() => onSelect({ type: 'messages' })}
                style={{ background: 'none', border: 'none', color: 'inherit', cursor: 'pointer', font: 'inherit', padding: 0, textTransform: 'inherit', letterSpacing: 'inherit', fontWeight: 'inherit' }}
              >
                Messages
              </button>
            </h3>
            {directMessages.map((c: ConversationSummary) => {
              const isSelected = selectedView.type === 'conversation' && selectedView.id === c.conversation_id;
              return (
                <button
                  key={c.conversation_id}
                  className={`sidebar-dm ${isSelected ? 'selected' : ''}`}
                  onClick={() => onSelect({ type: 'conversation', id: c.conversation_id })}
                >
                  <div className="sidebar-dm-top">
                    <span className="sidebar-dm-participants">
                      {c.participants.join(', ')}
                    </span>
                    <span className="sidebar-dm-time">
                      {formatRelativeTime(c.last_message_at)}
                    </span>
                  </div>
                  <div className="sidebar-dm-preview">{c.preview}</div>
                </button>
              );
            })}
          </section>
        </>
      )}
    </aside>
  );
}
