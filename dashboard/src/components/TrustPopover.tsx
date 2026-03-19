import { useState, useRef, useEffect } from 'react';
import { TrustShield, trustMeta } from './TrustShield';

const TRUST_DESCRIPTIONS: Record<number, string> = {
  0: 'No established trust. Messages are accepted but restricted.',
  1: 'You recognize this agent. Basic messaging allowed.',
  2: 'A known friend. Can participate in shared conversations.',
  3: 'Highly trusted. Can wake your agent and join projects.',
  4: 'Inner circle. Full access including sensitive operations.',
};

export function TrustPopover({
  level,
  name,
  onChangeTrust,
}: {
  level: number;
  name: string;
  onChangeTrust: (newLevel: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  return (
    <div className="trust-popover-anchor" ref={ref}>
      <TrustShield level={level} size={20} onClick={() => setOpen(!open)} />
      {open && (
        <div className="trust-popover">
          <div className="trust-popover-header">
            Trust level for {name}
          </div>
          <div className="trust-popover-current">
            <TrustShield level={level} size={24} />
            <div>
              <div className="trust-popover-name">{trustMeta(level).name}</div>
              <div className="trust-popover-desc">{TRUST_DESCRIPTIONS[level]}</div>
            </div>
          </div>
          <div className="trust-popover-options">
            {[0, 1, 2, 3, 4].map((l) => (
              <button
                key={l}
                className={`trust-option ${l === level ? 'active' : ''}`}
                onClick={() => { onChangeTrust(l); setOpen(false); }}
              >
                <TrustShield level={l} size={16} />
                <span>{trustMeta(l).name}</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
