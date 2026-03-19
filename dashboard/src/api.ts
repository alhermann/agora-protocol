// Fetch wrappers for all Agora daemon HTTP API endpoints.
// All requests go through Vite's /api proxy → http://127.0.0.1:7313

import type {
  StatusResponse,
  HealthResponse,
  PeersResponse,
  FriendsResponse,
  FriendRequestsResponse,
  InboxMessage,
  ConsumersResponse,
  RegisterConsumerResponse,
  SendRequest,
  SendResponse,
  WakeResponse,
  ConversationsResponse,
  ConversationResponse,
  ProjectsResponse,
  ProjectDetailResponse,
  ProjectInvitationsResponse,
  TaskListResponse,
  AuditListResponse,
  StageResponse,
  ProjectConversationResponse,
} from './types';

const BASE = '/api';

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) throw new Error(`GET ${path}: ${res.status}`);
  return res.json();
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`POST ${path}: ${res.status}`);
  return res.json();
}

async function del<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`DELETE ${path}: ${res.status}`);
  return res.json();
}

async function patchReq<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`PATCH ${path}: ${res.status}`);
  return res.json();
}

// --- Status & Health ---

export const getStatus = () => get<StatusResponse>('/status');
export const getHealth = () => get<HealthResponse>('/health');

// --- Peers ---

export const getPeers = () => get<PeersResponse>('/peers');

// --- Friends ---

export const getFriends = () => get<FriendsResponse>('/friends');

export const addFriend = (name: string, trustLevel?: number, alias?: string, notes?: string) =>
  post('/friends', { name, trust_level: trustLevel, alias, notes });

export const removeFriend = (name: string) =>
  del(`/friends/${encodeURIComponent(name)}?disconnect=true`);

export const disconnectPeer = (name: string) =>
  post<{ status: string; name: string }>(`/peers/${encodeURIComponent(name)}/disconnect`, {});

export const updateFriend = (name: string, fields: { muted?: boolean; trust_level?: number; alias?: string; notes?: string }) =>
  patchReq(`/friends/${encodeURIComponent(name)}`, fields);

// --- Messages (legacy) ---

export const getMessages = (wait = false, timeout = 30) =>
  get<InboxMessage[]>(`/messages?wait=${wait}&timeout=${timeout}`);

// --- Consumers ---

export const listConsumers = () => get<ConsumersResponse>('/consumers');

export const registerConsumer = (label: string) =>
  post<RegisterConsumerResponse>('/consumers', { label });

export const getConsumerMessages = (id: number, wait = false, timeout = 10) =>
  get<InboxMessage[]>(`/consumers/${id}/messages?wait=${wait}&timeout=${timeout}`);

export const unregisterConsumer = (id: number) => del(`/consumers/${id}`);

// --- Send ---

export const sendMessage = (req: SendRequest) => post<SendResponse>('/send', req);

// --- Wake ---

export const getWake = () => get<WakeResponse>('/wake');
export const setWake = (command: string | null) => post<WakeResponse>('/wake', { command });

// --- Connect ---

export const connectToPeer = (address: string) =>
  post<{ status: string; address: string }>('/connect', { address });

// --- Friend Requests ---

export const getFriendRequests = (status?: string) =>
  get<FriendRequestsResponse>(`/friend-requests${status ? `?status=${status}` : ''}`);

export const sendFriendRequest = (peerName: string, trustLevel?: number, message?: string) =>
  post('/friend-requests', { peer_name: peerName, trust_level: trustLevel, message });

export const acceptFriendRequest = (id: string, trustLevel?: number, message?: string) =>
  post(`/friend-requests/${encodeURIComponent(id)}/accept`, { trust_level: trustLevel, message });

export const rejectFriendRequest = (id: string, reason?: string) =>
  post(`/friend-requests/${encodeURIComponent(id)}/reject`, { reason });

// --- Conversations (available after Phase B) ---

export const getConversations = () =>
  get<ConversationsResponse>('/conversations').catch(() => ({ count: 0, conversations: [] }));

export const getConversation = (id: string) =>
  get<ConversationResponse>(`/conversations/${encodeURIComponent(id)}`);

export const deleteConversation = (id: string) =>
  del(`/conversations/${encodeURIComponent(id)}`);

export const deleteMessage = (conversationId: string, messageId: string) =>
  del(`/conversations/${encodeURIComponent(conversationId)}/messages/${encodeURIComponent(messageId)}`);

// --- Projects ---

export const getProjects = () =>
  get<ProjectsResponse>('/projects').catch(() => ({ count: 0, projects: [] }));

export const getProject = (id: string) =>
  get<ProjectDetailResponse>(`/projects/${encodeURIComponent(id)}`);

export const createProject = (name: string, description?: string, repo?: string) =>
  post('/projects', { name, description, repo });

export const projectClockIn = (id: string, focus?: string) =>
  post(`/projects/${encodeURIComponent(id)}/clock-in`, { focus });

export const projectClockOut = (id: string) =>
  post(`/projects/${encodeURIComponent(id)}/clock-out`, {});

export const getProjectInvitations = (status?: string) =>
  get<ProjectInvitationsResponse>(`/project-invitations${status ? `?status=${status}` : ''}`).catch(() => ({ count: 0, invitations: [] }));

export const sendProjectInvitation = (projectId: string, peerName: string, role: string, message?: string) =>
  post('/project-invitations', { project_id: projectId, peer_name: peerName, role, message });

export const acceptProjectInvitation = (id: string) =>
  post(`/project-invitations/${encodeURIComponent(id)}/accept`, {});

export const declineProjectInvitation = (id: string, reason?: string) =>
  post(`/project-invitations/${encodeURIComponent(id)}/decline`, { reason });

// --- Tasks ---

export const getProjectTasks = (projectId: string) =>
  get<TaskListResponse>(`/projects/${encodeURIComponent(projectId)}/tasks`).catch(() => ({ count: 0, tasks: [] }));

export const createTask = (projectId: string, title: string, description?: string, assignee?: string, priority?: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/tasks`, { title, description, assignee, priority });

export const updateTask = (projectId: string, taskId: string, fields: { status?: string; title?: string; description?: string; assignee?: string }) =>
  patchReq(`/projects/${encodeURIComponent(projectId)}/tasks/${encodeURIComponent(taskId)}`, fields);

export const deleteTask = (projectId: string, taskId: string) =>
  del(`/projects/${encodeURIComponent(projectId)}/tasks/${encodeURIComponent(taskId)}`);

// --- Audit ---

export const getProjectAudit = (projectId: string, offset = 0, limit = 100) =>
  get<AuditListResponse>(`/projects/${encodeURIComponent(projectId)}/audit?offset=${offset}&limit=${limit}`).catch(() => ({ count: 0, total: 0, entries: [] }));

// --- Stage ---

export const getProjectStage = (projectId: string) =>
  get<StageResponse>(`/projects/${encodeURIComponent(projectId)}/stage`).catch(() => ({ current_stage: null, stage_index: null, stages: [], can_advance: false }));

export const advanceProjectStage = (projectId: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/stage`, { advance: true });

export const setProjectStage = (projectId: string, stage: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/stage`, { stage });

// --- Project Conversations ---

export const getProjectConversations = (projectId: string) =>
  get<ProjectConversationResponse>(`/projects/${encodeURIComponent(projectId)}/conversations`).catch(() => ({ project_id: projectId, count: 0, messages: [] }));

// --- Suspend/Unsuspend ---

export const suspendAgent = (projectId: string, agentName: string, reason?: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentName)}/suspend`, { reason });

export const unsuspendAgent = (projectId: string, agentName: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentName)}/unsuspend`, {});

export const setAgentRole = (projectId: string, agentName: string, role: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentName)}/role`, { role });

export const removeProjectAgent = (projectId: string, agentName: string) =>
  del(`/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentName)}`);

// --- Rooms ---

import type { ProjectRoomsResponse, CreateRoomResponse, RoomSendResponse } from './types';

export const getProjectRooms = (projectId: string) =>
  get<ProjectRoomsResponse>(`/projects/${encodeURIComponent(projectId)}/rooms`).catch(() => ({ project_id: projectId, count: 0, rooms: [] }));

export const createRoom = (projectId: string, name: string, topic?: string) =>
  post<CreateRoomResponse>(`/projects/${encodeURIComponent(projectId)}/rooms`, { name, topic });

export const sendToRoom = (projectId: string, roomId: string, body: string) =>
  post<RoomSendResponse>(`/projects/${encodeURIComponent(projectId)}/rooms/${encodeURIComponent(roomId)}/send`, { body });

export const sendToMainRoom = (projectId: string, body: string) =>
  post<RoomSendResponse>(`/projects/${encodeURIComponent(projectId)}/rooms/main/send`, { body });

// --- Mute/Unmute ---

export const muteAgent = (projectId: string, agentName: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentName)}/mute`, {});

export const unmuteAgent = (projectId: string, agentName: string) =>
  post(`/projects/${encodeURIComponent(projectId)}/agents/${encodeURIComponent(agentName)}/unmute`, {});

// --- GitHub ---

import type { GitHubSyncResponse, GitHubStatusResponse } from './types';

export const githubSync = (projectId: string) =>
  post<GitHubSyncResponse>(`/projects/${encodeURIComponent(projectId)}/github/sync`, {});

export const getGitHubStatus = (projectId: string) =>
  get<GitHubStatusResponse>(`/projects/${encodeURIComponent(projectId)}/github/status`)
    .catch(() => ({ has_token: false, repo_url: null, parsed_repo: null, github_linked_tasks: 0, local_only_tasks: 0 }));
