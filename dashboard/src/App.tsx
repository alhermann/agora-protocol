import { useState } from 'react';
import { HeaderBar } from './components/HeaderBar';
import { Sidebar } from './components/Sidebar';
import { MainContent } from './components/MainContent';
import { ToastProvider } from './components/Toast';
import type { ViewState } from './types';

export default function App() {
  const [view, setView] = useState<ViewState>({ type: 'welcome' });
  const [sidebarOpen, setSidebarOpen] = useState(false);

  const handleSelect = (v: ViewState) => {
    setView(v);
    setSidebarOpen(false); // close on mobile after selection
  };

  return (
    <ToastProvider>
      <div className="app">
        <HeaderBar onHome={() => setView({ type: 'welcome' })} />
        <button
          className="hamburger"
          onClick={() => setSidebarOpen(!sidebarOpen)}
          aria-label="Toggle sidebar"
        >
          <span /><span /><span />
        </button>
        <div className="app-body">
          <div className={`sidebar-wrapper ${sidebarOpen ? 'open' : ''}`}>
            <Sidebar selectedView={view} onSelect={handleSelect} />
          </div>
          <main className="main-content">
            <MainContent view={view} onSelect={handleSelect} />
          </main>
        </div>
      </div>
    </ToastProvider>
  );
}
