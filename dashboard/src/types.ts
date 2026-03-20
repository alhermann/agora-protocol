// Types matching the Agora daemon HTTP API responses

export interface StatusResponse {
  version: string;
  node_name: string;
  peers_connected: number;
  running: boolean;
  did?: string;
  session_id?: string;
  owner_did?: string;
  wake_enabled?: boolean;
  wake_armed?: boolean;
  wake_listener_count?: number;
  wake_listener_labels?: string[];
  last_wake_at?: string | null;
  last_wake_from?: string | null;
  last_wake_message_count?: number | null;
}

export interface HealthResponse {
  healthy: boolean;
  uptime_seconds: number;
}

export interface PeerEntry {
  name: string;
  address: string;
  connected_at: string;
  did?: string;
  session_id?: string;
  verified: boolean;
  owner_did?: string;
  owner_verified?: boolean;
}

export interface PeersResponse {
  count: number;
  peers: PeerEntry[];
}

export interface FriendEntry {
  name: string;
  alias: string | null;
  trust_level: number;
  trust_name: string;
  can_wake: boolean;
  added_at: string;
  notes: string | null;
  muted: boolean;
  did?: string;
  last_address?: string;
  owner_did?: string;
  their_trust?: number;
  their_trust_name?: string;
}

export interface FriendsResponse {
  count: number;
  friends: FriendEntry[];
}

export interface InboxMessage {
  id?: string;
  from: string;
  body: string;
  timestamp: string;
  reply_to?: string | null;
  conversation_id?: string | null;
}

export interface ConsumerEntry {
  consumer_id: number;
  label: string;
  registered_at: string;
  last_active: string;
  buffered_messages: number;
}

export interface ConsumersResponse {
  count: number;
  consumers: ConsumerEntry[];
}

export interface RegisterConsumerResponse {
  consumer_id: number;
  label: string;
}

export interface SendRequest {
  body: string;
  to?: string;
  reply_to?: string;
  conversation_id?: string;
}

export interface SendResponse {
  status: string;
  id?: string;
}

export interface WakeResponse {
  command: string | null;
}

export interface ConversationSummary {
  conversation_id: string;
  message_count: number;
  participants: string[];
  first_message_at: string;
  last_message_at: string;
  preview: string;
}

export interface ConversationsResponse {
  count: number;
  conversations: ConversationSummary[];
}

export interface ConversationResponse {
  conversation_id: string;
  message_count: number;
  messages: StoredMessage[];
}

export interface StoredMessage {
  id: string;
  from: string;
  body: string;
  timestamp: string;
  reply_to: string | null;
  conversation_id: string | null;
  direction: string;
  project_id?: string | null;
}

export interface ProjectConversationResponse {
  project_id: string;
  count: number;
  messages: StoredMessage[];
}

export interface FriendRequestEntry {
  id: string;
  peer_name: string;
  peer_did?: string;
  offered_trust: number;
  offered_trust_name: string;
  direction: string;
  status: string;
  created_at: string;
  resolved_at?: string;
  message?: string;
  owner_did?: string;
}

export interface FriendRequestsResponse {
  count: number;
  requests: FriendRequestEntry[];
}

// Project types

export interface ProjectEntry {
  id: string;
  name: string;
  description?: string;
  owner_name: string;
  repo?: string;
  status: string;
  agent_count: number;
  active_agents: number;
  agent_names?: string[];
  created_at: string;
  updated_at: string;
}

export interface ProjectsResponse {
  count: number;
  projects: ProjectEntry[];
}

export interface ProjectAgent {
  name: string;
  did?: string;
  role: string;
  joined_at: string;
  clocked_in: boolean;
  current_focus?: string;
  last_clock_in?: string;
}

export interface ProjectDetailResponse {
  id: string;
  name: string;
  description?: string;
  owner_did: string;
  owner_name: string;
  repo?: string;
  status: string;
  agents: ProjectAgent[];
  created_at: string;
  updated_at: string;
  notes?: string;
}

export interface ProjectInvitationEntry {
  id: string;
  project_id: string;
  project_name: string;
  peer_name: string;
  role: string;
  direction: string;
  status: string;
  created_at: string;
  resolved_at?: string;
  message?: string;
}

export interface ProjectInvitationsResponse {
  count: number;
  invitations: ProjectInvitationEntry[];
}

// Task types

export interface TaskEntry {
  id: string;
  title: string;
  description?: string;
  status: string;
  assignee?: string;
  priority?: string;
  depends_on: string[];
  created_at: string;
  updated_at: string;
  created_by?: string;
}

export interface TaskListResponse {
  count: number;
  tasks: TaskEntry[];
}

// Audit types

export interface AuditEntryWire {
  id: string;
  timestamp: string;
  author_did: string;
  author_name: string;
  action: string;
  detail: string;
  signature: string;
}

export interface AuditListResponse {
  count: number;
  total: number;
  entries: AuditEntryWire[];
}

// Stage types

export interface StageResponse {
  current_stage: string | null;
  stage_index: number | null;
  stages: string[];
  can_advance: boolean;
}

// Room types

export interface ProjectRoomEntry {
  room_id: string;
  name: string;
  topic?: string;
  conversation_id: string;
  created_at: string;
  created_by: string;
}

export interface ProjectRoomsResponse {
  project_id: string;
  count: number;
  rooms: ProjectRoomEntry[];
}

export interface CreateRoomResponse {
  room_id: string;
  name: string;
  conversation_id: string;
}

export interface RoomSendResponse {
  status: string;
  id: string;
  room_id?: string;
  room_name?: string;
  conversation_id: string;
}

// GitHub integration types

export interface GitHubSyncResponse {
  status: string;
  imported: number;
  pushed: number;
  errors: string[];
}

export interface GitHubStatusResponse {
  has_token: boolean;
  repo_url: string | null;
  parsed_repo: string | null;
  github_linked_tasks: number;
  local_only_tasks: number;
}

// View state for sidebar → main content routing

export type ViewState =
  | { type: 'welcome' }
  | { type: 'conversation'; id: string }
  | { type: 'agent'; name: string }
  | { type: 'friend-requests' }
  | { type: 'projects' }
  | { type: 'project'; id: string }
  | { type: 'agents' }
  | { type: 'messages' }
  | { type: 'network' }
  | { type: 'threads' }
  | { type: 'thread'; id: string };

// Activity log — unified event timeline for the monitor dashboard

export type ActivityEventType =
  | 'message_in'
  | 'message_out'
  | 'peer_connected'
  | 'peer_disconnected'
  | 'wake_fired'
  | 'system';

export interface ActivityEvent {
  id: string;
  timestamp: string;
  type: ActivityEventType;
  from?: string;
  to?: string;
  body: string;
  conversationId?: string;
}
