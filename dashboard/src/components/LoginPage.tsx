import { useState } from 'react';

interface LoginPageProps {
  onLogin: (token: string) => void;
}

export default function LoginPage({ onLogin }: LoginPageProps) {
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');

    try {
      const base = localStorage.getItem('agora_api_base') || '';
      const resp = await fetch(`${base}/api/auth/verify`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${token}`,
        },
      });

      if (resp.ok) {
        localStorage.setItem('agora_api_token', token);
        onLogin(token);
      } else {
        setError('Invalid token. Check your token and try again.');
      }
    } catch {
      setError('Cannot connect to Agora daemon. Is it running?');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div style={{
      display: 'flex',
      justifyContent: 'center',
      alignItems: 'center',
      minHeight: '100vh',
      background: '#0a0a0a',
      color: '#e0e0e0',
    }}>
      <form onSubmit={handleSubmit} style={{
        background: '#1a1a2e',
        padding: '2rem',
        borderRadius: '8px',
        width: '400px',
        boxShadow: '0 4px 24px rgba(0,0,0,0.3)',
      }}>
        <h2 style={{ marginTop: 0, marginBottom: '1.5rem', textAlign: 'center' }}>
          Agora Dashboard
        </h2>
        <p style={{ color: '#888', fontSize: '0.9rem', marginBottom: '1rem' }}>
          Enter your API token to access the dashboard.
          Find it with: <code>agora token show</code>
        </p>
        <input
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="API Token"
          style={{
            width: '100%',
            padding: '0.75rem',
            marginBottom: '1rem',
            background: '#16213e',
            border: '1px solid #333',
            borderRadius: '4px',
            color: '#e0e0e0',
            fontSize: '1rem',
            boxSizing: 'border-box',
          }}
        />
        {error && (
          <p style={{ color: '#ff6b6b', fontSize: '0.85rem', margin: '0 0 1rem' }}>
            {error}
          </p>
        )}
        <button
          type="submit"
          disabled={loading || !token}
          style={{
            width: '100%',
            padding: '0.75rem',
            background: '#4a9eff',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
            fontSize: '1rem',
            cursor: loading ? 'wait' : 'pointer',
            opacity: loading || !token ? 0.6 : 1,
          }}
        >
          {loading ? 'Verifying...' : 'Login'}
        </button>
      </form>
    </div>
  );
}
