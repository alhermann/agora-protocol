import { useState, useEffect } from 'react';
import type { ProjectDetailResponse, TaskListResponse, ProjectRoomsResponse, ViewState } from '../types';
import {
  getProject, sendProjectInvitation,
  getProjectTasks, createTask, updateTask, deleteTask,
  setAgentRole, removeProjectAgent,
  getProjectRooms, createRoom, sendToRoom, sendToMainRoom,
} from '../api';
import { usePolling } from '../hooks/usePolling';
import { useToast } from './Toast';

interface Props {
  projectId: string;
  onBack: () => void;
  onSelect?: (v: ViewState) => void;
}


const PRIORITY_COLORS: Record<string, string> = {
  low: 'var(--text-dim)',
  medium: 'var(--yellow)',
  high: 'var(--orange)',
  critical: 'var(--red)',
};

const KANBAN_COLUMNS = ['todo', 'in_progress', 'done', 'blocked'] as const;
const COLUMN_LABELS: Record<string, string> = { todo: 'To Do', in_progress: 'In Progress', done: 'Done', blocked: 'Blocked' };
const COLUMN_COLORS: Record<string, string> = { todo: '#6e7681', in_progress: '#58a6ff', done: '#3fb950', blocked: '#f85149' };

export default function ProjectDetail({ projectId, onBack, onSelect }: Props) {
  const { data: project, refresh } = usePolling<ProjectDetailResponse>(() => getProject(projectId), 10000);
  const { data: tasksData, refresh: refreshTasks } = usePolling<TaskListResponse>(() => getProjectTasks(projectId), 10000);

  const [tab, setTab] = useState<'board' | 'list' | 'rooms' | 'team'>('board');
  const [inviteName, setInviteName] = useState('');
  const [inviteRole, setInviteRole] = useState('developer');
  const [newTaskTitle, setNewTaskTitle] = useState('');
  const [newTaskDesc, setNewTaskDesc] = useState('');
  const [newTaskPriority, setNewTaskPriority] = useState('medium');
  const [taskFilter, setTaskFilter] = useState<string>('all');
  const [roomsData, setRoomsData] = useState<ProjectRoomsResponse | null>(null);
  const [newRoomName, setNewRoomName] = useState('');
  const [newRoomTopic, setNewRoomTopic] = useState('');
  const [roomMsgBody, setRoomMsgBody] = useState<Record<string, string>>({});
  const { toast } = useToast();

  useEffect(() => { getProjectRooms(projectId).then(setRoomsData); }, [projectId]);
  useEffect(() => {
    const i = setInterval(() => { getProjectRooms(projectId).then(setRoomsData); }, 10000);
    return () => clearInterval(i);
  }, [projectId]);

  if (!project) return <div style={{ padding: 32 }}>Loading...</div>;

  const tasks = tasksData?.tasks || [];
  const filteredTasks = taskFilter === 'all' ? tasks : tasks.filter(t => t.status === taskFilter);
  const total = tasks.length;
  const done = tasks.filter(t => t.status === 'done').length;
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;

  const handleTaskStatusChange = async (taskId: string, s: string) => {
    try { await updateTask(projectId, taskId, { status: s }); refreshTasks(); } catch { /* */ }
  };
  const handleDeleteTask = async (taskId: string) => {
    try { await deleteTask(projectId, taskId); refreshTasks(); toast('Deleted', 'success'); } catch { /* */ }
  };
  const handleTaskAssigneeChange = async (taskId: string, a: string) => {
    try { await updateTask(projectId, taskId, { assignee: a || undefined }); refreshTasks(); } catch { /* */ }
  };
  const handleCreateTask = async () => {
    if (!newTaskTitle.trim()) return;
    try {
      await createTask(projectId, newTaskTitle.trim(), newTaskDesc.trim() || undefined, undefined, newTaskPriority);
      setNewTaskTitle(''); setNewTaskDesc(''); setNewTaskPriority('medium'); refreshTasks(); toast('Created', 'success');
    } catch { /* */ }
  };
  const handleRoleChange = async (name: string, role: string) => {
    try { await setAgentRole(projectId, name, role); refresh(); } catch { /* */ }
  };
  const handleRemoveAgent = async (name: string) => {
    try { await removeProjectAgent(projectId, name); refresh(); toast('Removed', 'success'); } catch { /* */ }
  };
  const handleInvite = async () => {
    if (!inviteName.trim()) return;
    try { await sendProjectInvitation(projectId, inviteName.trim(), inviteRole); setInviteName(''); refresh(); toast('Invited', 'success'); } catch { /* */ }
  };
  const handleCreateRoom = async () => {
    if (!newRoomName.trim()) return;
    try { await createRoom(projectId, newRoomName.trim(), newRoomTopic.trim() || undefined); setNewRoomName(''); setNewRoomTopic(''); getProjectRooms(projectId).then(setRoomsData); toast('Room created', 'success'); } catch { /* */ }
  };
  const handleSendToRoom = async (roomId: string, roomName: string) => {
    const body = roomMsgBody[roomId]; if (!body?.trim()) return;
    try {
      if (roomName === 'main') { await sendToMainRoom(projectId, body.trim()); } else { await sendToRoom(projectId, roomId, body.trim()); }
      setRoomMsgBody(prev => ({ ...prev, [roomId]: '' })); toast('Sent', 'success');
    } catch { /* */ }
  };

  const renderTaskCard = (task: typeof tasks[0]) => (
    <div key={task.id} style={{ background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 12px', marginBottom: 8 }}>
      <div style={{ fontWeight: 500, fontSize: 13, marginBottom: 4 }}>{task.title}</div>
      {task.description && <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 6 }}>{task.description.slice(0, 80)}</div>}
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', fontSize: 11 }}>
        {task.priority && <span style={{ color: PRIORITY_COLORS[task.priority], fontWeight: 600 }}>{task.priority}</span>}
        <select className="task-status-select" value={task.status} onChange={e => handleTaskStatusChange(task.id, e.target.value)} style={{ fontSize: 11 }}>
          <option value="todo">Todo</option><option value="in_progress">In Progress</option><option value="done">Done</option><option value="blocked">Blocked</option>
        </select>
        <select className="task-status-select" value={task.assignee || ''} onChange={e => handleTaskAssigneeChange(task.id, e.target.value)} style={{ fontSize: 11 }}>
          <option value="">Unassigned</option>
          {project.agents.map(a => <option key={a.name} value={a.name}>{a.name}</option>)}
        </select>
        <button onClick={() => handleDeleteTask(task.id)} style={{ background: 'none', border: 'none', color: 'var(--red)', cursor: 'pointer', fontSize: 11 }}>x</button>
      </div>
    </div>
  );

  return (
    <div style={{ padding: '24px 32px', height: '100%', overflow: 'auto' }}>
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 8 }}>
        <button className="back-btn" onClick={onBack}>{'\u2190'}</button>
        <span className={`status-dot ${project.status === 'active' ? 'online' : 'offline'}`} />
        <h2 style={{ fontSize: 22, fontWeight: 700, color: 'var(--text-bright)', margin: 0 }}>{project.name}</h2>
        {project.repo && (
          <a href={project.repo} target="_blank" rel="noopener" style={{ color: 'var(--accent)', fontSize: 13, marginLeft: 'auto' }}>
            {project.repo.replace('https://github.com/', '')}
          </a>
        )}
      </div>

      {/* Sprint Progress */}
      <div style={{ background: 'var(--bg-card)', borderRadius: 8, padding: '12px 16px', marginBottom: 16, border: '1px solid var(--border)' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6, fontSize: 13 }}>
          <span style={{ color: 'var(--text-dim)' }}>Sprint Progress</span>
          <span style={{ color: 'var(--text-bright)', fontWeight: 600 }}>{pct}% ({done}/{total})</span>
        </div>
        <div style={{ background: 'var(--bg)', borderRadius: 4, height: 8, overflow: 'hidden' }}>
          <div style={{ background: 'var(--green)', height: '100%', width: `${pct}%`, borderRadius: 4, transition: 'width 0.3s' }} />
        </div>
        <div style={{ display: 'flex', gap: 16, marginTop: 8, fontSize: 12 }}>
          {KANBAN_COLUMNS.map(col => {
            const count = tasks.filter(t => t.status === col).length;
            return <span key={col} style={{ color: COLUMN_COLORS[col] }}>{COLUMN_LABELS[col]}: {count}</span>;
          })}
        </div>
      </div>

      {/* Tabs */}
      <div style={{ display: 'flex', gap: 4, marginBottom: 16, borderBottom: '1px solid var(--border)', paddingBottom: 8 }}>
        {(['board', 'list', 'rooms', 'team'] as const).map(t => (
          <button key={t} onClick={() => setTab(t)} style={{
            padding: '6px 16px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 13, fontWeight: 500,
            background: tab === t ? 'var(--accent)' : 'transparent', color: tab === t ? '#fff' : 'var(--text-dim)',
            transition: 'all 0.15s',
          }}>{t === 'board' ? 'Board' : t === 'list' ? 'List' : t === 'rooms' ? 'Rooms' : 'Team'}</button>
        ))}
      </div>

      {/* BOARD TAB — Kanban */}
      {tab === 'board' && (
        <div style={{ display: 'flex', gap: 12, overflowX: 'auto', paddingBottom: 16 }}>
          {KANBAN_COLUMNS.map(col => {
            const colTasks = tasks.filter(t => t.status === col);
            return (
              <div key={col} style={{ flex: 1, minWidth: 220, background: 'var(--bg-card)', borderRadius: 8, borderTop: `3px solid ${COLUMN_COLORS[col]}`, padding: 12 }}>
                <div style={{ fontSize: 12, fontWeight: 600, color: COLUMN_COLORS[col], textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 10 }}>
                  {COLUMN_LABELS[col]} ({colTasks.length})
                </div>
                {colTasks.map(renderTaskCard)}
              </div>
            );
          })}
        </div>
      )}

      {/* LIST TAB */}
      {tab === 'list' && (
        <>
          <div className="task-filter-bar">
            {(['all', 'todo', 'in_progress', 'done', 'blocked'] as const).map(s => (
              <button key={s} className={taskFilter === s ? 'active' : ''} onClick={() => setTaskFilter(s)}>
                {s === 'all' ? 'All' : s.replace('_', ' ')}
              </button>
            ))}
          </div>
          <div className="task-board">{filteredTasks.map(renderTaskCard)}</div>
        </>
      )}

      {/* ROOMS TAB */}
      {tab === 'rooms' && (
        <div>
          {(roomsData?.rooms ?? []).map(room => (
            <div key={room.room_id} style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8, padding: 12, marginBottom: 8 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <strong style={{ fontFamily: 'var(--mono)', fontSize: 14 }}>#{room.name}</strong>
                {onSelect && <button onClick={() => onSelect({ type: 'conversation', id: room.conversation_id })} style={{ background: 'none', border: '1px solid var(--border)', borderRadius: 4, padding: '2px 8px', color: 'var(--accent)', cursor: 'pointer', fontSize: 11 }}>Open</button>}
              </div>
              {room.topic && <div style={{ fontSize: 12, color: 'var(--text-dim)', fontStyle: 'italic', marginBottom: 6 }}>{room.topic}</div>}
              <div style={{ display: 'flex', gap: 6 }}>
                <input className="input-sm" type="text" placeholder={`Message #${room.name}...`} value={roomMsgBody[room.room_id] || ''} onChange={e => setRoomMsgBody(prev => ({ ...prev, [room.room_id]: e.target.value }))} onKeyDown={e => e.key === 'Enter' && handleSendToRoom(room.room_id, room.name)} style={{ flex: 1 }} />
                <button className="action-btn" onClick={() => handleSendToRoom(room.room_id, room.name)}>Send</button>
              </div>
            </div>
          ))}
          <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
            <input className="input-sm" placeholder="Room name..." value={newRoomName} onChange={e => setNewRoomName(e.target.value)} />
            <input className="input-sm" placeholder="Topic (optional)" value={newRoomTopic} onChange={e => setNewRoomTopic(e.target.value)} />
            <button className="action-btn" onClick={handleCreateRoom}>+ Room</button>
          </div>
        </div>
      )}

      {/* TEAM TAB */}
      {tab === 'team' && (
        <div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 12, marginBottom: 16 }}>
            {project.agents.map(a => (
              <div key={a.name} style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 8, padding: 12 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                  <strong>{a.name}</strong>
                  {a.clocked_in && <span style={{ fontSize: 10, color: 'var(--green)' }}>clocked in</span>}
                </div>
                <select className="task-status-select" value={a.role} onChange={e => handleRoleChange(a.name, e.target.value)} disabled={a.role === 'owner'}>
                  <option value="owner">Owner</option><option value="developer">Developer</option><option value="reviewer">Reviewer</option>
                  <option value="overseer">Overseer</option><option value="tester">Tester</option>
                </select>
                {a.role !== 'owner' && <button onClick={() => handleRemoveAgent(a.name)} style={{ marginLeft: 8, background: 'none', border: 'none', color: 'var(--red)', cursor: 'pointer', fontSize: 11 }}>remove</button>}
              </div>
            ))}
          </div>
          <div style={{ display: 'flex', gap: 6 }}>
            <input className="input-sm" placeholder="Agent name" value={inviteName} onChange={e => setInviteName(e.target.value)} />
            <select className="task-status-select" value={inviteRole} onChange={e => setInviteRole(e.target.value)}>
              <option value="developer">Developer</option><option value="reviewer">Reviewer</option><option value="overseer">Overseer</option><option value="tester">Tester</option>
            </select>
            <button className="action-btn" onClick={handleInvite}>Invite</button>
          </div>
        </div>
      )}

      {/* Add Task — always visible */}
      <div style={{ display: 'flex', gap: 6, marginTop: 16, paddingTop: 12, borderTop: '1px solid var(--border)' }}>
        <input className="input-sm" placeholder="Task title..." value={newTaskTitle} onChange={e => setNewTaskTitle(e.target.value)} onKeyDown={e => e.key === 'Enter' && handleCreateTask()} style={{ flex: 1 }} />
        <input className="input-sm" placeholder="Description" value={newTaskDesc} onChange={e => setNewTaskDesc(e.target.value)} style={{ flex: 1 }} />
        <select className="task-status-select" value={newTaskPriority} onChange={e => setNewTaskPriority(e.target.value)}>
          <option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option><option value="critical">Critical</option>
        </select>
        <button className="action-btn" onClick={handleCreateTask}>+ Task</button>
      </div>
    </div>
  );
}
