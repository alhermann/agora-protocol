import { useCallback, useEffect, useRef, useState } from 'react';
import Markdown from 'react-markdown';
import { usePolling } from '../hooks/usePolling';
import {
  closeThread,
  getConversation,
  getStatus,
  getThread,
  sendMessage,
} from '../api';
import { useToast } from './Toast';
import type { ConversationResponse, StatusResponse, StoredMessage } from '../types';
import type { ThreadDetail as ThreadDetailResponse } from '../api';

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleString([], {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

const SENDER_COLORS = [
  '#50c878',
  '#53c0f0',
  '#f0c050',
  '#e07050',
  '#b070e0',
  '#50d0d0',
  '#e0a050',
  '#70b0e0',
];

function senderColor(name: string, participants: string[]): string {
  const idx = participants.indexOf(name);
  return SENDER_COLORS[idx >= 0 ? idx % SENDER_COLORS.length : 0];
}

export function ThreadDetail({
  threadId,
  onBack,
}: {
  threadId: string;
  onBack?: () => void;
}) {
  const fetchThread = useCallback(() => getThread(threadId), [threadId]);
  const fetchConversation = useCallback(() => getConversation(threadId), [threadId]);
  const fetchStatus = useCallback(() => getStatus(), []);

  const { data: thread, error: threadError, refresh: refreshThread } =
    usePolling<ThreadDetailResponse>(fetchThread, 10000);
  const { data: conversation, error: conversationError, refresh: refreshConversation } =
    usePolling<ConversationResponse>(fetchConversation, 10000);
  const { data: status } = usePolling<StatusResponse>(fetchStatus, 10000);
  const { toast } = useToast();

  const bottomRef = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);
  const [closing, setClosing] = useState(false);

  const myName = status?.node_name ?? '';
  const messages = conversation?.messages ?? [];

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length]);

  const handleSend = async () => {
    if (!draft.trim() || sending || thread?.closed) return;
    setSending(true);
    try {
      await sendMessage({
        body: draft.trim(),
        conversation_id: threadId,
      });
      setDraft('');
      refreshConversation();
    } catch (e) {
      toast(`Failed to send to thread: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error');
    }
    setSending(false);
  };

  const handleClose = async () => {
    if (!thread || thread.closed || closing) return;
    if (!confirm(`Close thread "${thread.title || 'Untitled thread'}"?`)) return;

    setClosing(true);
    try {
      await closeThread(threadId);
      refreshThread();
      toast('Thread closed', 'success');
    } catch (e) {
      toast(`Failed to close thread: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error');
    }
    setClosing(false);
  };

  if (threadError || conversationError) {
    return <div className="main-placeholder">Could not load thread.</div>;
  }

  if (!thread || !conversation) {
    return <div className="main-placeholder">Loading thread...</div>;
  }

  const participants = [...new Set(messages.map((m) => m.from))];
  thread.participants.forEach((name) => {
    if (!participants.includes(name)) {
      participants.push(name);
    }
  });

  return (
    <div className="conversation-chat">
      <div
        className="chat-header"
        style={{
          alignItems: 'flex-start',
          gap: 16,
          paddingTop: 18,
          paddingBottom: 18,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0, flex: 1 }}>
          {onBack && <button className="back-btn" onClick={onBack} title="Back to threads">{'\u2190'}</button>}
          <div style={{ minWidth: 0, flex: 1 }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 10,
                flexWrap: 'wrap',
                marginBottom: 6,
              }}
            >
              <span className="chat-participants" style={{ fontSize: 20 }}>
                # {thread.title || 'Untitled thread'}
              </span>
              <span
                style={{
                  fontSize: 11,
                  color: thread.closed ? 'var(--yellow)' : 'var(--accent)',
                  background: 'rgba(56, 139, 253, 0.12)',
                  borderRadius: 999,
                  padding: '4px 8px',
                  textTransform: 'uppercase',
                  letterSpacing: '0.06em',
                }}
              >
                {thread.closed ? 'Closed' : 'Open'}
              </span>
            </div>
            <div
              style={{
                display: 'flex',
                gap: 14,
                flexWrap: 'wrap',
                fontSize: 12,
                color: 'var(--text-dim)',
              }}
            >
              <span>Created by {thread.creator}</span>
              <span>{thread.participant_count} participant{thread.participant_count === 1 ? '' : 's'}</span>
              <span>{formatDate(thread.created_at)}</span>
              <span>{conversation.message_count} message{conversation.message_count === 1 ? '' : 's'}</span>
            </div>
          </div>
        </div>

        {!thread.closed && thread.creator === myName && (
          <button
            className="delete-conversation-btn"
            onClick={handleClose}
            disabled={closing}
            title="Close thread"
          >
            {closing ? 'Closing...' : 'Close thread'}
          </button>
        )}
      </div>

      <div
        style={{
          padding: '0 24px 16px',
          borderBottom: '1px solid var(--border)',
          display: 'flex',
          flexWrap: 'wrap',
          gap: 8,
        }}
      >
        {thread.participants.map((participant) => (
          <span
            key={participant}
            style={{
              padding: '6px 10px',
              borderRadius: 999,
              border: '1px solid var(--border)',
              background: participant === myName ? 'rgba(56, 139, 253, 0.12)' : 'var(--bg-card)',
              color: participant === myName ? 'var(--text-bright)' : 'var(--text-dim)',
              fontSize: 12,
            }}
          >
            {participant}
          </span>
        ))}
      </div>

      <div className="chat-messages">
        {messages.length === 0 ? (
          <div className="main-placeholder" style={{ minHeight: 220 }}>
            No messages in this thread yet. Use it like a shared channel for focused discussion.
          </div>
        ) : (
          messages.map((msg: StoredMessage) => {
            const outbound = msg.from === myName;
            const color = outbound ? 'var(--accent)' : senderColor(msg.from, participants);
            return (
              <div key={msg.id} className={`chat-bubble ${outbound ? 'outbound' : 'inbound'}`}>
                <div className="bubble-header">
                  <span className="bubble-sender" style={{ color }}>{msg.from}</span>
                  <span className="bubble-time">{formatTime(msg.timestamp)}</span>
                </div>
                <div className="bubble-body"><Markdown>{msg.body}</Markdown></div>
              </div>
            );
          })
        )}
        <div ref={bottomRef} />
      </div>

      <div className="chat-compose">
        <input
          type="text"
          placeholder={thread.closed ? 'Thread is closed' : `Message #${thread.title || 'thread'}`}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSend()}
          disabled={sending || thread.closed}
        />
        <button onClick={handleSend} disabled={sending || thread.closed || !draft.trim()}>
          {thread.closed ? 'Closed' : 'Send'}
        </button>
      </div>
    </div>
  );
}
