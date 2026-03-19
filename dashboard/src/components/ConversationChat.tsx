import { useCallback, useRef, useEffect, useState } from 'react';
import Markdown from 'react-markdown';
import { usePolling } from '../hooks/usePolling';
import { getConversation, sendMessage, deleteConversation, deleteMessage, getStatus } from '../api';
import { useToast } from './Toast';
import type { StatusResponse, ConversationResponse, StoredMessage } from '../types';

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

const SENDER_COLORS = [
  '#50c878', // green
  '#53c0f0', // blue
  '#f0c050', // gold
  '#e07050', // coral
  '#b070e0', // purple
  '#50d0d0', // teal
  '#e0a050', // orange
  '#70b0e0', // sky
];

function senderColor(name: string, participants: string[]): string {
  const idx = participants.indexOf(name);
  return SENDER_COLORS[idx >= 0 ? idx % SENDER_COLORS.length : 0];
}

export function ConversationChat({ conversationId, onBack }: { conversationId: string; onBack?: () => void }) {
  const fetcher = useCallback(() => getConversation(conversationId), [conversationId]);
  const fetchStatus = useCallback(() => getStatus(), []);
  const { data, error, refresh } = usePolling<ConversationResponse>(fetcher, 10000);
  const { data: status } = usePolling<StatusResponse>(fetchStatus, 10000);
  const { toast } = useToast();
  const myName = status?.node_name ?? '';

  const bottomRef = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);

  const handleDeleteConversation = async () => {
    if (!confirm('Delete this entire conversation?')) return;
    try {
      await deleteConversation(conversationId);
      toast('Conversation deleted', 'success');
      onBack?.();
    } catch (e) {
      toast(`Failed to delete conversation: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error');
    }
  };

  const handleDeleteMessage = async (messageId: string) => {
    try {
      await deleteMessage(conversationId, messageId);
      refresh();
    } catch (e) {
      toast(`Failed to delete message: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error');
    }
  };

  // Auto-scroll to bottom
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [data?.messages.length]);

  // Derive "to" from conversation participants (the other party)
  const otherParticipant = data?.messages.find(m => m.direction === 'inbound')?.from;

  const handleSend = async () => {
    if (!draft.trim() || sending) return;
    setSending(true);
    try {
      await sendMessage({
        body: draft.trim(),
        to: otherParticipant,
        conversation_id: conversationId,
      });
      setDraft('');
      refresh();
    } catch (e) {
      toast(`Failed to send: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error');
    }
    setSending(false);
  };

  if (error) {
    return <div className="main-placeholder">Could not load conversation.</div>;
  }

  if (!data) {
    return <div className="main-placeholder">Loading conversation...</div>;
  }

  return (
    <div className="conversation-chat">
      <div className="chat-header">
        {onBack && <button className="back-btn" onClick={onBack} title="Back to overview">{'\u2190'}</button>}
        <span className="chat-participants">
          {data.messages.length > 0
            ? [...new Set(data.messages.map((m) => m.from))].join(' \u2194 ')
            : 'Conversation'}
        </span>
        <span className="chat-meta">{data.message_count} message{data.message_count !== 1 ? 's' : ''}</span>
        <button className="delete-conversation-btn" onClick={handleDeleteConversation} title="Delete conversation">Delete</button>
      </div>
      <div className="chat-messages">
        {(() => {
          const participants = [...new Set(data.messages.map(m => m.from))];
          return data.messages.map((msg: StoredMessage) => {
          const outbound = msg.from === myName;
          const color = outbound ? 'var(--accent)' : senderColor(msg.from, participants);
          return (
            <div key={msg.id} className={`chat-bubble ${outbound ? 'outbound' : 'inbound'}`}>
              <div className="bubble-header">
                <span className="bubble-sender" style={{ color }}>{msg.from}</span>
                <span className="bubble-time">{formatTime(msg.timestamp)}</span>
                <button
                  className="delete-msg-btn"
                  onClick={() => handleDeleteMessage(msg.id)}
                  title="Delete message"
                >
                  &times;
                </button>
              </div>
              <div className="bubble-body"><Markdown>{msg.body}</Markdown></div>
            </div>
          );
        });
        })()}
        <div ref={bottomRef} />
      </div>
      <div className="chat-compose">
        <input
          type="text"
          placeholder="Type a message..."
          value={draft}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && handleSend()}
          disabled={sending}
        />
        <button onClick={handleSend} disabled={sending || !draft.trim()}>
          Send
        </button>
      </div>
    </div>
  );
}
