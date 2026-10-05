import React, { useEffect, useRef, useState } from 'react';
import type { LogEntry } from '../logger';
import { logger } from '../logger';
import { Terminal, Trash2, ChevronDown, ChevronUp, Copy, Check } from 'lucide-react';

export const DiagnosticConsole: React.FC = () => {
  const [entries, setEntries] = useState<LogEntry[]>(() => logger.getEntries());
  const [isExpanded, setIsExpanded] = useState<boolean>(true);
  const [copied, setCopied] = useState<boolean>(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unsubscribe = logger.subscribe(() => {
      setEntries(logger.getEntries());
    });
    return unsubscribe;
  }, []);

  useEffect(() => {
    if (isExpanded) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [entries, isExpanded]);

  const copyLogs = () => {
    const text = entries
      .map((e) => `[${e.timestamp}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`)
      .join('\n');
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const levelColor: Record<string, string> = {
    info: 'text-[var(--bb-smoke)]',
    ipc: 'text-[var(--bb-gold)]',
    warn: 'text-[var(--bb-orange)]',
    error: 'text-[var(--bb-crimson)] font-bold',
    success: 'text-[var(--bb-amber)] font-bold',
  };

  const levelBadge: Record<string, string> = {
    info: 'bg-[var(--bb-panel)] text-[var(--bb-smoke)] border-[var(--bb-border)]',
    ipc: 'bg-[var(--bb-panel)] text-[var(--bb-gold)] border-[var(--bb-border-bright)]',
    warn: 'bg-[var(--bb-panel)] text-[var(--bb-orange)] border-[var(--bb-orange)]',
    error: 'bg-[var(--bb-ember-dark)] text-[var(--bb-white)] border-[var(--bb-crimson)]',
    success: 'bg-[var(--bb-panel)] text-[var(--bb-amber)] border-[var(--bb-gold)]',
  };

  return (
    <div className="w-full bg-[var(--bb-vacuum)] border-t border-[var(--bb-border)] flex flex-col transition-all">
      {/* Console Header Bar */}
      <div className="h-8 bg-[var(--bb-panel)] px-3 flex items-center justify-between border-b border-[var(--bb-border)] text-xs select-none">
        <div className="flex items-center gap-2">
          <Terminal className="w-3.5 h-3.5 text-[var(--bb-gold)]" />
          <span className="font-bold tracking-wider text-[var(--bb-sand)] text-[11px]">
            TELEMETRY LOG STREAM
          </span>
          <span className="px-1.5 py-0.2 text-[9px] bg-[var(--bb-surface)] text-[var(--bb-smoke)] border border-[var(--bb-border)]">
            {entries.length} EVENTS
          </span>
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={copyLogs}
            className="px-2 py-0.5 bg-[var(--bb-surface)] hover:bg-[var(--bb-panel)] text-[10px] text-[var(--bb-sand)] border border-[var(--bb-border)] flex items-center gap-1 transition-colors"
          >
            {copied ? <Check className="w-3 h-3 text-[var(--bb-amber)]" /> : <Copy className="w-3 h-3" />}
            {copied ? 'COPIED' : 'COPY'}
          </button>
          <button
            type="button"
            onClick={() => logger.clear()}
            className="px-2 py-0.5 bg-[var(--bb-surface)] hover:bg-[var(--bb-panel)] text-[10px] text-[var(--bb-sand)] border border-[var(--bb-border)] flex items-center gap-1 transition-colors"
          >
            <Trash2 className="w-3 h-3" /> CLEAR
          </button>
          <button
            type="button"
            onClick={() => setIsExpanded(!isExpanded)}
            className="px-2 py-0.5 bg-[var(--bb-surface)] hover:bg-[var(--bb-panel)] text-[10px] text-[var(--bb-gold)] border border-[var(--bb-border)] flex items-center gap-1 transition-colors"
          >
            {isExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronUp className="w-3 h-3" />}
            {isExpanded ? 'COLLAPSE' : 'EXPAND'}
          </button>
        </div>
      </div>

      {/* Console Log Feed */}
      {isExpanded && (
        <div className="h-36 overflow-y-auto p-2.5 font-mono text-[11px] leading-relaxed space-y-1 bg-[var(--bb-space)]/90 select-text">
          {entries.map((entry) => (
            <div key={entry.id} className="flex items-start gap-2 hover:bg-[var(--bb-surface)]/60 px-1 py-0.5 rounded-xs">
              <span className="text-[10px] text-[var(--bb-ash)] shrink-0 select-none">
                {entry.timestamp}
              </span>
              <span className={`px-1 py-0.2 text-[9px] uppercase border shrink-0 select-none ${levelBadge[entry.level] || ''}`}>
                {entry.level}
              </span>
              <span className="text-[10px] text-[var(--bb-smoke)] shrink-0 font-bold select-none">
                [{entry.source}]
              </span>
              <span className={`flex-1 break-all ${levelColor[entry.level] || 'text-[var(--bb-sand)]'}`}>
                {entry.message}
                {entry.data !== undefined && (
                  <pre className="text-[9px] text-[var(--bb-smoke)] mt-0.5 bg-[var(--bb-panel)]/80 p-1 border border-[var(--bb-border)] overflow-x-auto">
                    {typeof entry.data === 'string' ? entry.data : JSON.stringify(entry.data, null, 2)}
                  </pre>
                )}
              </span>
            </div>
          ))}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  );
};
