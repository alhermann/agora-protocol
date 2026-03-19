import type { ViewState } from '../types';
import { WelcomeView } from './WelcomeView';
import { ConversationChat } from './ConversationChat';
import { AgentDetail } from './AgentDetail';
import { FriendRequests } from './FriendRequests';
import { ProjectsOverview } from './ProjectsOverview';
import { AgentsOverview } from './AgentsOverview';
import { MessagesOverview } from './MessagesOverview';
import ProjectDetail from './ProjectDetail';

export function MainContent({
  view,
  onSelect,
}: {
  view: ViewState;
  onSelect: (v: ViewState) => void;
}) {
  const goHome = () => onSelect({ type: 'welcome' });

  switch (view.type) {
    case 'conversation':
      return <ConversationChat conversationId={view.id} onBack={goHome} />;
    case 'agent':
      return <AgentDetail name={view.name} onSelect={onSelect} />;
    case 'friend-requests':
      return <FriendRequests onBack={goHome} />;
    case 'projects':
      return <ProjectsOverview onSelect={onSelect} />;
    case 'agents':
      return <AgentsOverview onSelect={onSelect} />;
    case 'messages':
      return <MessagesOverview onSelect={onSelect} />;
    case 'project':
      return <ProjectDetail projectId={view.id} onBack={() => onSelect({ type: 'projects' })} onSelect={onSelect} />;
    case 'welcome':
    default:
      return <WelcomeView onSelect={onSelect} />;
  }
}
