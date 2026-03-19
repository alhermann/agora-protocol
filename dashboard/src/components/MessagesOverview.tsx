import { useCallback } from 'react';
import { usePolling } from '../hooks/usePolling';
import { getConversations, getProjects, getProjectRooms } from '../api';
import type { ConversationsResponse, ProjectsResponse, ViewState } from '../types';
import { useState, useEffect } from 'react';

function formatRelativeTime(isoStr: string): string {
  const diff = (Date.now() - new Date(isoStr).getTime()) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

interface RoomInfo {
  roomName: string;
  projectName: string;
  projectId: string;
}

export function MessagesOverview({ onSelect }: { onSelect: (v: ViewState) => void }) {
  const { data: convos } = usePolling<ConversationsResponse>(useCallback(() => getConversations(), []), 10000);
  const { data: projects } = usePolling<ProjectsResponse>(useCallback(() => getProjects(), []), 10000);
  const [roomMap, setRoomMap] = useState<Record<string, RoomInfo>>({});

  // Build room conversation ID map
  useEffect(() => {
    if (!projects) return;
    const active = projects.projects.filter(p => p.status === 'active');
    active.forEach(p => {
      getProjectRooms(p.id).then(rooms => {
        const map: Record<string, RoomInfo> = {};
        rooms.rooms.forEach(r => {
          map[r.conversation_id] = { roomName: r.name, projectName: p.name, projectId: p.id };
        });
        setRoomMap(prev => ({ ...prev, ...map }));
      });
    });
  }, [projects]);

  const conversations = convos?.conversations ?? [];

  // Group: project rooms vs direct messages
  const projectGroups = new Map<string, { projectName: string; projectId: string; rooms: typeof conversations }>();
  const directMessages: typeof conversations = [];

  conversations.forEach(c => {
    const room = roomMap[c.conversation_id];
    if (room) {
      if (!projectGroups.has(room.projectId)) {
        projectGroups.set(room.projectId, { projectName: room.projectName, projectId: room.projectId, rooms: [] });
      }
      projectGroups.get(room.projectId)!.rooms.push(c);
    } else {
      directMessages.push(c);
    }
  });

  return (
    <div style={{ padding: 32, height: '100%', overflow: 'auto' }}>
      <h2 style={{ fontSize: 24, fontWeight: 700, color: 'var(--text-bright)', marginBottom: 24 }}>Messages</h2>

      {/* Project Rooms */}
      {[...projectGroups.entries()].map(([pid, group]) => (
        <div key={pid} style={{ marginBottom: 24 }}>
          <h3
            style={{ fontSize: 14, color: 'var(--accent)', marginBottom: 10, cursor: 'pointer' }}
            onClick={() => onSelect({ type: 'project', id: pid })}
          >
            {group.projectName}
          </h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {group.rooms.sort((a, b) => b.last_message_at.localeCompare(a.last_message_at)).map(c => {
              const room = roomMap[c.conversation_id];
              return (
                <button
                  key={c.conversation_id}
                  onClick={() => onSelect({ type: 'conversation', id: c.conversation_id })}
                  style={{
                    background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8,
                    padding: '10px 14px', cursor: 'pointer', textAlign: 'left', transition: 'all 0.15s',
                    display: 'flex', alignItems: 'center', gap: 12,
                  }}
                  onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
                  onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
                >
                  <span style={{ fontFamily: 'var(--mono)', fontSize: 13, color: 'var(--text-bright)', minWidth: 120 }}>
                    #{room?.roomName ?? '?'}
                  </span>
                  <span style={{ fontSize: 12, color: 'var(--text-dim)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {c.preview}
                  </span>
                  <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>
                    {c.message_count} msgs
                  </span>
                  <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>
                    {formatRelativeTime(c.last_message_at)}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      ))}

      {/* Direct Messages */}
      {directMessages.length > 0 && (
        <div style={{ marginBottom: 24 }}>
          <h3 style={{ fontSize: 14, color: 'var(--text-dim)', marginBottom: 10, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
            Direct Messages
          </h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {directMessages.sort((a, b) => b.last_message_at.localeCompare(a.last_message_at)).map(c => (
              <button
                key={c.conversation_id}
                onClick={() => onSelect({ type: 'conversation', id: c.conversation_id })}
                style={{
                  background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8,
                  padding: '10px 14px', cursor: 'pointer', textAlign: 'left', transition: 'all 0.15s',
                  display: 'flex', alignItems: 'center', gap: 12,
                }}
                onMouseOver={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)'; }}
                onMouseOut={e => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
              >
                <span style={{ fontSize: 13, color: 'var(--text-bright)', minWidth: 120 }}>
                  {c.participants.join(', ')}
                </span>
                <span style={{ fontSize: 12, color: 'var(--text-dim)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {c.preview}
                </span>
                <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>
                  {c.message_count} msgs
                </span>
                <span style={{ fontSize: 11, color: 'var(--text-dim)', flexShrink: 0 }}>
                  {formatRelativeTime(c.last_message_at)}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}

      {conversations.length === 0 && (
        <div style={{ color: 'var(--text-dim)', fontSize: 14 }}>No messages yet. Send a message in a project room to start.</div>
      )}
    </div>
  );
}
