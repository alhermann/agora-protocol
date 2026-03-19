import { useState, useEffect, useRef, useCallback } from 'react';
import { usePolling } from './usePolling';
import { getPeers, getHealth, getConversations, getConversation, getStatus } from '../api';
import type { PeersResponse, HealthResponse, ConversationsResponse, ActivityEvent, StatusResponse } from '../types';

let eventCounter = 0;
function nextId(prefix: string): string {
  return `${prefix}-${++eventCounter}`;
}

/**
 * Merges peer connection diffs + system events + conversation-based message
 * detection into a unified sorted ActivityEvent[] for the activity feed.
 *
 * No consumer registration — polls GET /conversations instead.
 */
export function useActivityLog() {
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const prevPeersRef = useRef<Set<string>>(new Set());
  const startupEmitted = useRef(false);
  const prevMsgCountRef = useRef<Map<string, number>>(new Map());
  const initialLoadRef = useRef(true);
  const wakeInitRef = useRef(false);
  const lastWakeAtRef = useRef<string | null>(null);

  const fetchPeers = useCallback(() => getPeers(), []);
  const fetchHealth = useCallback(() => getHealth(), []);
  const fetchConversations = useCallback(() => getConversations(), []);
  const fetchStatus = useCallback(() => getStatus(), []);

  const { data: peersData, error: peersError } = usePolling<PeersResponse>(fetchPeers, 3000);
  const { data: healthData } = usePolling<HealthResponse>(fetchHealth, 3000);
  const { data: convosData } = usePolling<ConversationsResponse>(fetchConversations, 3000);
  const { data: statusData } = usePolling<StatusResponse>(fetchStatus, 3000);

  // Emit daemon started event from uptime
  useEffect(() => {
    if (!healthData || startupEmitted.current) return;
    startupEmitted.current = true;

    const startedAt = new Date(Date.now() - healthData.uptime_seconds * 1000).toISOString();
    setEvents((prev) => [
      {
        id: nextId('sys'),
        timestamp: startedAt,
        type: 'system',
        body: 'Daemon started',
      },
      ...prev,
    ]);
  }, [healthData]);

  // Detect peer connection/disconnection changes
  useEffect(() => {
    if (!peersData) return;

    const currentNames = new Set(peersData.peers.map((p) => p.name));
    const prevNames = prevPeersRef.current;

    const now = new Date().toISOString();
    const newEvents: ActivityEvent[] = [];

    for (const name of currentNames) {
      if (!prevNames.has(name)) {
        const peer = peersData.peers.find((p) => p.name === name);
        newEvents.push({
          id: nextId('peer'),
          timestamp: peer?.connected_at ?? now,
          type: 'peer_connected',
          from: name,
          body: `${name} connected`,
        });
      }
    }

    for (const name of prevNames) {
      if (!currentNames.has(name)) {
        newEvents.push({
          id: nextId('peer'),
          timestamp: now,
          type: 'peer_disconnected',
          from: name,
          body: `${name} disconnected`,
        });
      }
    }

    prevPeersRef.current = currentNames;

    if (newEvents.length > 0) {
      setEvents((prev) => [...prev, ...newEvents]);
    }
  }, [peersData]);

  // Detect new messages by diffing conversation message counts
  useEffect(() => {
    if (!convosData) return;

    const prevCounts = prevMsgCountRef.current;
    const newCounts = new Map<string, number>();
    const conversationsWithNewMessages: string[] = [];

    for (const c of convosData.conversations) {
      newCounts.set(c.conversation_id, c.message_count);
      const prev = prevCounts.get(c.conversation_id) ?? 0;
      if (c.message_count > prev && !initialLoadRef.current) {
        conversationsWithNewMessages.push(c.conversation_id);
      }
    }

    prevMsgCountRef.current = newCounts;

    // On initial load, just record counts — don't flood activity feed with history
    if (initialLoadRef.current) {
      initialLoadRef.current = false;
      return;
    }

    // Fetch new messages from conversations that grew
    for (const convId of conversationsWithNewMessages) {
      const prevCount = prevCounts.get(convId) ?? 0;
      getConversation(convId).then((resp) => {
        const newMsgs = resp.messages.slice(prevCount);
        const msgEvents: ActivityEvent[] = newMsgs.map((m) => ({
          id: m.id || nextId('msg'),
          timestamp: m.timestamp,
          type: m.direction === 'outbound' ? 'message_out' as const : 'message_in' as const,
          from: m.from,
          body: m.body,
          conversationId: m.conversation_id ?? undefined,
        }));
        if (msgEvents.length > 0) {
          setEvents((prev) => [...prev, ...msgEvents]);
        }
      }).catch(() => {});
    }
  }, [convosData]);

  // Emit wake events when the daemon reports a new wake firing.
  useEffect(() => {
    if (!statusData) return;

    const current = statusData.last_wake_at ?? null;
    if (!wakeInitRef.current) {
      wakeInitRef.current = true;
      lastWakeAtRef.current = current;
      return;
    }
    if (!current || current === lastWakeAtRef.current) return;

    lastWakeAtRef.current = current;
    const count = statusData.last_wake_message_count ?? 1;
    const from = statusData.last_wake_from;
    setEvents((prev) => [
      ...prev,
      {
        id: nextId('wake'),
        timestamp: current,
        type: 'wake_fired',
        from: from ?? undefined,
        body: `Wake fired for ${count} message${count === 1 ? '' : 's'}`,
      },
    ]);
  }, [statusData]);

  // Return sorted events (oldest first)
  const sorted = [...events].sort(
    (a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime()
  );

  return { events: sorted, error: peersError };
}
