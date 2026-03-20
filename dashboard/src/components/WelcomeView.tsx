import { useCallback, useEffect, useState } from 'react';
import { usePolling } from '../hooks/usePolling';
import { useActivityLog } from '../hooks/useActivityLog';
import {
  getFriendRequests,
  getFriends,
  getHealth,
  getProjectInvitations,
  getProjects,
  getProjectStage,
  getProjectTasks,
  getStatus,
  getThreads,
  listConsumers,
} from '../api';
import type { ThreadSummary } from '../api';
import type {
  ConsumersResponse,
  FriendRequestsResponse,
  FriendsResponse,
  HealthResponse,
  ProjectInvitationsResponse,
  ProjectsResponse,
  StatusResponse,
  ViewState,
} from '../types';

interface ProjectSnapshot {
  total: number;
  done: number;
  todo: number;
  inProgress: number;
  blocked: number;
  stage: string | null;
  canAdvance: boolean;
}

interface AttentionItem {
  key: string;
  title: string;
  detail: string;
  tone: 'critical' | 'warn' | 'info';
  action: ViewState;
}

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

function formatStage(stage: string | null | undefined): string {
  if (!stage) return 'No stage';
  return stage
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function toneColor(tone: AttentionItem['tone']): string {
  switch (tone) {
    case 'critical':
      return 'var(--red)';
    case 'warn':
      return 'var(--yellow)';
    case 'info':
    default:
      return 'var(--accent)';
  }
}

function eventTone(type: string): string {
  switch (type) {
    case 'message_in':
      return 'var(--accent)';
    case 'message_out':
      return 'var(--green)';
    case 'peer_connected':
      return 'var(--green)';
    case 'peer_disconnected':
      return 'var(--red)';
    case 'wake_fired':
      return 'var(--yellow)';
    default:
      return 'var(--text-dim)';
  }
}

function eventLabel(type: string): string {
  switch (type) {
    case 'message_in':
      return 'message in';
    case 'message_out':
      return 'message out';
    case 'peer_connected':
      return 'peer connected';
    case 'peer_disconnected':
      return 'peer disconnected';
    case 'wake_fired':
      return 'wake';
    default:
      return 'system';
  }
}

export function WelcomeView({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { events } = useActivityLog();
  const { data: status } = usePolling<StatusResponse>(useCallback(() => getStatus(), []), 10000);
  const { data: health } = usePolling<HealthResponse>(useCallback(() => getHealth(), []), 10000);
  const { data: projects } = usePolling<ProjectsResponse>(useCallback(() => getProjects(), []), 10000);
  const { data: consumers } = usePolling<ConsumersResponse>(useCallback(() => listConsumers(), []), 10000);
  const { data: friends } = usePolling<FriendsResponse>(useCallback(() => getFriends(), []), 10000);
  const { data: threadsData } = usePolling<{ count: number; threads: ThreadSummary[] }>(
    useCallback(() => getThreads().catch(() => ({ count: 0, threads: [] })), []),
    10000,
  );
  const { data: friendRequests } = usePolling<FriendRequestsResponse>(
    useCallback(
      () => getFriendRequests('pending').catch(() => ({ count: 0, requests: [] })),
      [],
    ),
    10000,
  );
  const { data: projectInvitations } = usePolling<ProjectInvitationsResponse>(
    useCallback(
      () => getProjectInvitations('pending').catch(() => ({ count: 0, invitations: [] })),
      [],
    ),
    10000,
  );

  const [projectSnapshots, setProjectSnapshots] = useState<Record<string, ProjectSnapshot>>({});

  const activeProjects = projects?.projects.filter((project) => project.status === 'active') ?? [];
  const activeProjectsKey = activeProjects
    .map((project) => `${project.id}:${project.updated_at}`)
    .join('|');

  useEffect(() => {
    let cancelled = false;

    const loadProjectSnapshots = async () => {
      if (activeProjects.length === 0) {
        if (!cancelled) setProjectSnapshots({});
        return;
      }

      const entries = await Promise.all(
        activeProjects.map(async (project) => {
          const [tasksResult, stageResult] = await Promise.allSettled([
            getProjectTasks(project.id),
            getProjectStage(project.id),
          ]);

          const tasks =
            tasksResult.status === 'fulfilled' ? tasksResult.value.tasks ?? [] : [];
          const stage =
            stageResult.status === 'fulfilled' ? stageResult.value : null;

          return [
            project.id,
            {
              total: tasks.length,
              done: tasks.filter((task) => task.status === 'done').length,
              todo: tasks.filter((task) => task.status === 'todo').length,
              inProgress: tasks.filter((task) => task.status === 'in_progress').length,
              blocked: tasks.filter((task) => task.status === 'blocked').length,
              stage: stage?.current_stage ?? null,
              canAdvance: stage?.can_advance ?? false,
            },
          ] as const;
        }),
      );

      if (!cancelled) {
        setProjectSnapshots(Object.fromEntries(entries));
      }
    };

    void loadProjectSnapshots();
    const interval = setInterval(() => {
      void loadProjectSnapshots();
    }, 10000);

    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [activeProjectsKey]);

  const now = Date.now();
  const onlineAgents = new Set<string>();
  consumers?.consumers.forEach((consumer) => {
    if (
      consumer.label !== 'http-default' &&
      consumer.label !== status?.node_name &&
      now - new Date(consumer.last_active).getTime() < 60000
    ) {
      onlineAgents.add(consumer.label);
    }
  });

  const knownAgents = new Set<string>();
  friends?.friends.forEach((friend) => knownAgents.add(friend.name));
  activeProjects.forEach((project) => {
    (project.agent_names ?? []).forEach((name) => {
      if (name !== status?.node_name) {
        knownAgents.add(name);
      }
    });
  });

  const listenerLabels = new Set(status?.wake_listener_labels ?? []);
  const listenerConsumers = (consumers?.consumers ?? []).filter((consumer) =>
    listenerLabels.has(consumer.label),
  );
  const listenerBacklog = listenerConsumers.reduce(
    (sum, consumer) => sum + consumer.buffered_messages,
    0,
  );

  const activeThreads = [...(threadsData?.threads ?? [])]
    .filter((thread) => !thread.closed)
    .sort((a, b) => b.created_at.localeCompare(a.created_at));
  const recentEvents = [...events].slice(-6).reverse();
  const activeAgentEntries = [...(consumers?.consumers ?? [])]
    .filter((consumer) => consumer.label !== 'http-default' && !listenerLabels.has(consumer.label))
    .sort((a, b) => b.last_active.localeCompare(a.last_active))
    .slice(0, 5);

  const pendingFriendRequests =
    friendRequests?.requests.filter(
      (request) => request.status === 'pending' && request.direction === 'inbound',
    ) ?? [];
  const pendingProjectInvitations =
    projectInvitations?.invitations.filter(
      (invite) => invite.status === 'pending' && invite.direction === 'inbound',
    ) ?? [];

  const totalTasks = Object.values(projectSnapshots).reduce(
    (sum, snapshot) => sum + snapshot.total,
    0,
  );
  const totalDone = Object.values(projectSnapshots).reduce(
    (sum, snapshot) => sum + snapshot.done,
    0,
  );
  const totalBlocked = Object.values(projectSnapshots).reduce(
    (sum, snapshot) => sum + snapshot.blocked,
    0,
  );
  const overallPct = totalTasks > 0 ? Math.round((totalDone / totalTasks) * 100) : 0;

  const projectsByPriority = [...activeProjects].sort((a, b) => {
    const aSnapshot = projectSnapshots[a.id];
    const bSnapshot = projectSnapshots[b.id];
    return (
      (bSnapshot?.blocked ?? 0) - (aSnapshot?.blocked ?? 0) ||
      (bSnapshot?.inProgress ?? 0) - (aSnapshot?.inProgress ?? 0) ||
      b.updated_at.localeCompare(a.updated_at)
    );
  });

  const attentionItems: AttentionItem[] = [];
  if (!status?.running) {
    attentionItems.push({
      key: 'daemon-offline',
      title: 'Daemon is offline',
      detail: 'The local Agora daemon is not healthy enough to drive the dashboard.',
      tone: 'critical',
      action: { type: 'network' },
    });
  }
  if ((status?.wake_listener_count ?? 0) < 1) {
    attentionItems.push({
      key: 'listener-missing',
      title: 'No active listener',
      detail: 'Wake automation is armed without an attached listener process.',
      tone: 'critical',
      action: { type: 'network' },
    });
  } else if (listenerBacklog > 0) {
    attentionItems.push({
      key: 'listener-backlog',
      title: `${listenerBacklog} listener message${listenerBacklog === 1 ? '' : 's'} buffered`,
      detail: 'The automation consumer is behind. Check active threads and messages.',
      tone: 'warn',
      action: { type: 'messages' },
    });
  }
  if (totalBlocked > 0) {
    attentionItems.push({
      key: 'blocked-tasks',
      title: `${totalBlocked} blocked task${totalBlocked === 1 ? '' : 's'} across active projects`,
      detail: 'Blocked tasks are the clearest signal that human intervention may be needed.',
      tone: 'critical',
      action: { type: 'projects' },
    });
  }
  if (pendingProjectInvitations.length > 0) {
    attentionItems.push({
      key: 'project-invites',
      title: `${pendingProjectInvitations.length} pending project invitation${pendingProjectInvitations.length === 1 ? '' : 's'}`,
      detail: 'Review inbound project invitations before work starts drifting to DMs or side channels.',
      tone: 'info',
      action: { type: 'projects' },
    });
  }
  if (pendingFriendRequests.length > 0) {
    attentionItems.push({
      key: 'friend-requests',
      title: `${pendingFriendRequests.length} pending friend request${pendingFriendRequests.length === 1 ? '' : 's'}`,
      detail: 'New peers are waiting for trust decisions before they can collaborate cleanly.',
      tone: 'info',
      action: { type: 'friend-requests' },
    });
  }
  if (activeProjects.length > 0 && onlineAgents.size === 0) {
    attentionItems.push({
      key: 'nobody-online',
      title: 'No agents currently active',
      detail: 'Projects exist, but no recent agent consumer activity is visible.',
      tone: 'warn',
      action: { type: 'agents' },
    });
  }
  if (activeProjects.length === 0) {
    attentionItems.push({
      key: 'no-projects',
      title: 'No active projects',
      detail: 'The dashboard is running, but there is nothing active to monitor or coordinate.',
      tone: 'info',
      action: { type: 'projects' },
    });
  }

  const cardStyle = {
    background: 'var(--bg-card)',
    border: '1px solid var(--border)',
    borderRadius: 12,
    padding: 20,
    boxShadow: 'var(--shadow-sm)',
  } as const;

  const navButtonStyle = {
    background: 'var(--bg-card)',
    border: '1px solid var(--border)',
    color: 'var(--text)',
    borderRadius: 999,
    padding: '8px 12px',
    cursor: 'pointer',
    fontSize: 12,
  } as const;

  return (
    <div style={{ padding: 'clamp(16px, 3vw, 32px)', height: '100%', overflow: 'auto' }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          gap: 16,
          flexWrap: 'wrap',
          marginBottom: 24,
        }}
      >
        <div>
          <h2
            style={{
              fontSize: 26,
              fontWeight: 700,
              color: 'var(--text-bright)',
              marginBottom: 6,
            }}
          >
            Overview
          </h2>
          <div style={{ color: 'var(--text-dim)', fontSize: 14 }}>
            What needs attention across projects, agents, and active conversations.
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button style={navButtonStyle} onClick={() => onSelect({ type: 'projects' })}>
            Open projects
          </button>
          <button style={navButtonStyle} onClick={() => onSelect({ type: 'threads' })}>
            Open threads
          </button>
          <button style={navButtonStyle} onClick={() => onSelect({ type: 'messages' })}>
            Open messages
          </button>
        </div>
      </div>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
          gap: 16,
          marginBottom: 24,
        }}
      >
        <section style={cardStyle}>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 14,
            }}
          >
            <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>
              Needs Attention
            </h3>
            <span style={{ fontSize: 12, color: 'var(--text-dim)' }}>
              {attentionItems.length === 0 ? 'All clear' : `${attentionItems.length} item${attentionItems.length === 1 ? '' : 's'}`}
            </span>
          </div>

          {attentionItems.length === 0 ? (
            <div
              style={{
                border: '1px solid rgba(52, 208, 88, 0.2)',
                background: 'rgba(52, 208, 88, 0.08)',
                borderRadius: 10,
                padding: 16,
                color: 'var(--text)',
                fontSize: 14,
              }}
            >
              No urgent operator actions right now. Projects are flowing and the listener is keeping up.
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              {attentionItems.map((item) => (
                <button
                  key={item.key}
                  onClick={() => onSelect(item.action)}
                  style={{
                    width: '100%',
                    textAlign: 'left',
                    background: 'rgba(255,255,255,0.01)',
                    border: `1px solid ${toneColor(item.tone)}33`,
                    borderRadius: 10,
                    padding: '14px 16px',
                    cursor: 'pointer',
                    color: 'var(--text)',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center',
                      gap: 12,
                      marginBottom: 4,
                    }}
                  >
                    <span
                      style={{
                        fontSize: 14,
                        fontWeight: 600,
                        color: 'var(--text-bright)',
                      }}
                    >
                      {item.title}
                    </span>
                    <span
                      style={{
                        fontSize: 11,
                        color: toneColor(item.tone),
                        textTransform: 'uppercase',
                        letterSpacing: '0.05em',
                      }}
                    >
                      {item.tone}
                    </span>
                  </div>
                  <div style={{ fontSize: 12, color: 'var(--text-dim)', lineHeight: 1.5 }}>
                    {item.detail}
                  </div>
                </button>
              ))}
            </div>
          )}
        </section>

        <section style={cardStyle}>
          <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)', marginBottom: 14 }}>
            System Snapshot
          </h3>

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
              gap: 10,
              marginBottom: 14,
            }}
          >
            {[
              {
                label: 'Daemon',
                value: status?.running ? 'Online' : 'Offline',
                sub: health ? formatUptime(health.uptime_seconds) : '--',
                color: status?.running ? 'var(--green)' : 'var(--red)',
              },
              {
                label: 'Listener',
                value: (status?.wake_listener_count ?? 0) > 0 ? 'Active' : 'Missing',
                sub: listenerBacklog > 0 ? `${listenerBacklog} buffered` : 'caught up',
                color: (status?.wake_listener_count ?? 0) > 0 ? 'var(--accent)' : 'var(--red)',
              },
              {
                label: 'Agents',
                value: `${onlineAgents.size}`,
                sub: `${knownAgents.size} known`,
                color: 'var(--green)',
              },
              {
                label: 'Projects',
                value: `${activeProjects.length}`,
                sub: totalTasks > 0 ? `${overallPct}% complete` : 'no tasks yet',
                color: 'var(--text-bright)',
              },
            ].map((metric) => (
              <div
                key={metric.label}
                style={{
                  background: 'rgba(255,255,255,0.02)',
                  border: '1px solid var(--border)',
                  borderRadius: 10,
                  padding: 14,
                }}
              >
                <div
                  style={{
                    fontSize: 11,
                    color: 'var(--text-dim)',
                    textTransform: 'uppercase',
                    letterSpacing: '0.05em',
                    marginBottom: 6,
                  }}
                >
                  {metric.label}
                </div>
                <div style={{ fontSize: 22, fontWeight: 700, color: metric.color }}>
                  {metric.value}
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-dim)', marginTop: 4 }}>
                  {metric.sub}
                </div>
              </div>
            ))}
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, fontSize: 12 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
              <span style={{ color: 'var(--text-dim)' }}>Wake automation</span>
              <span style={{ color: status?.wake_enabled ? 'var(--green)' : 'var(--text-dim)' }}>
                {status?.wake_enabled ? 'enabled' : 'disabled'}
              </span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
              <span style={{ color: 'var(--text-dim)' }}>Connected peers</span>
              <span style={{ color: 'var(--text)' }}>{status?.peers_connected ?? 0}</span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
              <span style={{ color: 'var(--text-dim)' }}>Pending requests</span>
              <span style={{ color: 'var(--text)' }}>
                {pendingFriendRequests.length + pendingProjectInvitations.length}
              </span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
              <span style={{ color: 'var(--text-dim)' }}>Open threads</span>
              <span style={{ color: 'var(--text)' }}>{activeThreads.length}</span>
            </div>
          </div>

          <div
            style={{
              marginTop: 16,
              paddingTop: 14,
              borderTop: '1px solid var(--border)',
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                marginBottom: 10,
              }}
            >
              <div style={{ fontSize: 12, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                Recently Active Agents
              </div>
              <button
                onClick={() => onSelect({ type: 'agents' })}
                style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 12 }}
              >
                View all
              </button>
            </div>

            {activeAgentEntries.length === 0 ? (
              <div style={{ color: 'var(--text-dim)', fontSize: 12 }}>
                No recent agent consumer activity.
              </div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                {activeAgentEntries.map((consumer) => (
                  <button
                    key={consumer.consumer_id}
                    onClick={() => onSelect({ type: 'agent', name: consumer.label })}
                    style={{
                      width: '100%',
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center',
                      gap: 12,
                      background: 'rgba(255,255,255,0.01)',
                      border: '1px solid var(--border)',
                      borderRadius: 10,
                      padding: '10px 12px',
                      cursor: 'pointer',
                      color: 'var(--text)',
                    }}
                  >
                    <div style={{ minWidth: 0 }}>
                      <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-bright)' }}>
                        {consumer.label}
                      </div>
                      <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                        active {formatRelativeTime(consumer.last_active)}
                      </div>
                    </div>
                    <div style={{ fontSize: 11, color: consumer.buffered_messages > 0 ? 'var(--yellow)' : 'var(--text-dim)' }}>
                      {consumer.buffered_messages > 0
                        ? `${consumer.buffered_messages} buffered`
                        : 'caught up'}
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        </section>
      </div>

      <section style={{ ...cardStyle, marginBottom: 24 }}>
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            marginBottom: 14,
          }}
        >
          <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>
            Project Health
          </h3>
          <button
            onClick={() => onSelect({ type: 'projects' })}
            style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 12 }}
          >
            View all
          </button>
        </div>

        {projectsByPriority.length === 0 ? (
          <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>
            No active projects yet.
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {projectsByPriority.map((project) => {
              const snapshot = projectSnapshots[project.id];
              const progressPct =
                snapshot && snapshot.total > 0
                  ? Math.round((snapshot.done / snapshot.total) * 100)
                  : 0;

              return (
                <button
                  key={project.id}
                  onClick={() => onSelect({ type: 'project', id: project.id })}
                  style={{
                    width: '100%',
                    textAlign: 'left',
                    background: 'rgba(255,255,255,0.01)',
                    border: '1px solid var(--border)',
                    borderRadius: 10,
                    padding: '16px 18px',
                    cursor: 'pointer',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'flex-start',
                      gap: 16,
                      marginBottom: 10,
                    }}
                  >
                    <div style={{ minWidth: 0, flex: 1 }}>
                      <div
                        style={{
                          fontSize: 15,
                          fontWeight: 600,
                          color: 'var(--text-bright)',
                          marginBottom: 4,
                        }}
                      >
                        {project.name}
                      </div>
                      {project.description && (
                        <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>
                          {project.description.length > 100
                            ? `${project.description.slice(0, 100)}...`
                            : project.description}
                        </div>
                      )}
                    </div>

                    <div
                      style={{
                        fontSize: 11,
                        color: 'var(--text-dim)',
                        flexShrink: 0,
                      }}
                    >
                      updated {formatRelativeTime(project.updated_at)}
                    </div>
                  </div>

                  <div
                    style={{
                      display: 'flex',
                      flexWrap: 'wrap',
                      gap: 8,
                      marginBottom: 10,
                    }}
                  >
                    <span
                      style={{
                        padding: '4px 8px',
                        borderRadius: 999,
                        background: 'rgba(124, 106, 239, 0.12)',
                        color: 'var(--accent)',
                        fontSize: 11,
                      }}
                    >
                      {formatStage(snapshot?.stage)}
                    </span>
                    <span
                      style={{
                        padding: '4px 8px',
                        borderRadius: 999,
                        background: 'rgba(255,255,255,0.04)',
                        color: 'var(--text)',
                        fontSize: 11,
                      }}
                    >
                      {project.agent_count} agents
                    </span>
                    <span
                      style={{
                        padding: '4px 8px',
                        borderRadius: 999,
                        background: 'rgba(248, 81, 73, 0.12)',
                        color: snapshot?.blocked ? 'var(--red)' : 'var(--text-dim)',
                        fontSize: 11,
                      }}
                    >
                      {snapshot?.blocked ?? 0} blocked
                    </span>
                    <span
                      style={{
                        padding: '4px 8px',
                        borderRadius: 999,
                        background: 'rgba(227, 179, 65, 0.12)',
                        color: 'var(--yellow)',
                        fontSize: 11,
                      }}
                    >
                      {snapshot?.inProgress ?? 0} in progress
                    </span>
                    <span
                      style={{
                        padding: '4px 8px',
                        borderRadius: 999,
                        background: 'rgba(255,255,255,0.04)',
                        color: 'var(--text)',
                        fontSize: 11,
                      }}
                    >
                      {snapshot?.todo ?? 0} todo
                    </span>
                    {snapshot?.canAdvance && (
                      <span
                        style={{
                          padding: '4px 8px',
                          borderRadius: 999,
                          background: 'rgba(52, 208, 88, 0.12)',
                          color: 'var(--green)',
                          fontSize: 11,
                        }}
                      >
                        ready to advance
                      </span>
                    )}
                  </div>

                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 12,
                    }}
                  >
                    <div
                      style={{
                        flex: 1,
                        height: 6,
                        background: 'var(--bg)',
                        borderRadius: 999,
                        overflow: 'hidden',
                      }}
                    >
                      <div
                        style={{
                          width: `${progressPct}%`,
                          height: '100%',
                          background: 'var(--green)',
                          borderRadius: 999,
                        }}
                      />
                    </div>
                    <div style={{ fontSize: 12, color: 'var(--text-dim)', flexShrink: 0 }}>
                      {snapshot?.done ?? 0}/{snapshot?.total ?? 0} done
                    </div>
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </section>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
          gap: 16,
        }}
      >
        <section style={cardStyle}>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 14,
            }}
          >
            <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>
              Active Threads
            </h3>
            <button
              onClick={() => onSelect({ type: 'threads' })}
              style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 12 }}
            >
              View all
            </button>
          </div>

          {activeThreads.length === 0 ? (
            <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>
              No active threads right now.
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              {activeThreads.slice(0, 5).map((thread) => (
                <button
                  key={thread.id}
                  onClick={() => onSelect({ type: 'thread', id: thread.id })}
                  style={{
                    width: '100%',
                    textAlign: 'left',
                    background: 'rgba(255,255,255,0.01)',
                    border: '1px solid var(--border)',
                    borderRadius: 10,
                    padding: '14px 16px',
                    cursor: 'pointer',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      gap: 12,
                      marginBottom: 4,
                    }}
                  >
                    <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-bright)' }}>
                      # {thread.title || 'Untitled thread'}
                    </span>
                    <span style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                      {formatRelativeTime(thread.created_at)}
                    </span>
                  </div>
                  <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>
                    {thread.participant_count} participant{thread.participant_count === 1 ? '' : 's'} • created by {thread.creator}
                  </div>
                </button>
              ))}
            </div>
          )}
        </section>

        <section style={cardStyle}>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 14,
            }}
          >
            <h3 style={{ fontSize: 16, fontWeight: 600, color: 'var(--text-bright)' }}>
              Operator Feed
            </h3>
            <button
              onClick={() => onSelect({ type: 'messages' })}
              style={{ background: 'none', border: 'none', color: 'var(--accent)', cursor: 'pointer', fontSize: 12 }}
            >
              View all
            </button>
          </div>

          {recentEvents.length === 0 ? (
            <div style={{ color: 'var(--text-dim)', fontSize: 13 }}>
              No recent operator events yet.
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              {recentEvents.map((event) => (
                <button
                  key={event.id}
                  onClick={() =>
                    event.conversationId
                      ? onSelect({ type: 'conversation', id: event.conversationId })
                      : onSelect({ type: 'messages' })
                  }
                  style={{
                    width: '100%',
                    textAlign: 'left',
                    background: 'rgba(255,255,255,0.01)',
                    border: '1px solid var(--border)',
                    borderRadius: 10,
                    padding: '14px 16px',
                    cursor: 'pointer',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      gap: 12,
                      marginBottom: 4,
                    }}
                  >
                    <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-bright)' }}>
                      {event.from ?? 'System'}
                    </span>
                    <span style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                      {formatRelativeTime(event.timestamp)}
                    </span>
                  </div>
                  <div
                    style={{
                      fontSize: 10,
                      color: eventTone(event.type),
                      textTransform: 'uppercase',
                      letterSpacing: '0.06em',
                      marginBottom: 6,
                    }}
                  >
                    {eventLabel(event.type)}
                  </div>
                  <div
                    style={{
                      fontSize: 12,
                      color: 'var(--text-dim)',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                      marginBottom: 6,
                    }}
                  >
                    {event.body}
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                    {event.conversationId ? 'Open conversation' : 'Open messages'}
                  </div>
                </button>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
