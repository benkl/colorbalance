import React from 'react';
import type { BatchSummary } from '../types';
import type { BatchProgress } from '../tauri';

interface Props {
  summary: BatchSummary | null;
  /** Latest progress event while a batch runs, otherwise null. */
  progress: BatchProgress | null;
  running: boolean;
}

type Status = 'written' | 'skipped' | 'failed';

const STATUS_STYLE: Record<Status, string> = {
  written: 'text-[var(--bb-gold)]',
  skipped: 'text-[var(--bb-smoke)]',
  failed: 'text-[var(--bb-crimson)]',
};

const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/**
 * Per-file outcome of a batch. While it runs this shows the progress bar and
 * the file being processed; afterwards one row per file with its status, any
 * warning, and the failure reason.
 */
export const BatchResults: React.FC<Props> = ({ summary, progress, running }) => {
  const warningsByFile = new Map<string, string[]>();
  summary?.warnings.forEach(({ file, warning }) => {
    warningsByFile.set(file, [...(warningsByFile.get(file) ?? []), warning]);
  });
  const rows: { file: string; status: Status; detail: string[] }[] = summary
    ? [
        ...summary.succeeded.map((file) => ({ file, status: 'written' as const, detail: warningsByFile.get(file) ?? [] })),
        ...summary.skipped.map((file) => ({ file, status: 'skipped' as const, detail: ['Output exists; not overwritten.', ...(warningsByFile.get(file) ?? [])] })),
        ...summary.failed.map(({ file, error }) => ({ file, status: 'failed' as const, detail: [error] })),
      ]
    : [];

  const total = progress?.total ?? summary?.total ?? 0;
  const completed = running ? (progress?.completed ?? 0) : rows.length;

  return (
    <div className="w-full h-full flex flex-col min-h-0 p-4 gap-3" data-testid="batch-results">
      <div className="space-y-1.5">
        <div className="flex items-baseline justify-between text-[10px] text-[var(--bb-smoke)]">
          <span className="font-bold tracking-wider">{running ? 'WRITING BATCH' : 'BATCH FINISHED'}</span>
          <span className="tabular-nums" data-testid="batch-count">{completed}/{total} files</span>
        </div>
        <div className="h-1 bg-[var(--bb-border)]">
          <div
            className="h-full bg-[var(--bb-gold)] transition-all"
            style={{ width: `${total > 0 ? (completed / total) * 100 : 0}%` }}
            data-testid="batch-bar"
          />
        </div>
        {running && progress?.file && (
          <div className="text-[10px] text-[var(--bb-sand)] truncate">{baseName(progress.file)}</div>
        )}
      </div>

      {rows.length > 0 && (
        <ul className="flex-1 min-h-0 overflow-y-auto divide-y divide-[var(--bb-border)] border-y border-[var(--bb-border)]" data-testid="batch-files">
          {rows.map((row) => (
            <li key={`${row.status}:${row.file}`} className="py-1.5 px-1 text-[11px] flex gap-3">
              <span className={`w-16 shrink-0 text-[10px] font-bold uppercase ${STATUS_STYLE[row.status]}`}>{row.status}</span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[var(--bb-sand)]" title={row.file}>{baseName(row.file)}</span>
                {row.detail.map((line, i) => (
                  <span key={i} className={`block text-[10px] ${row.status === 'failed' ? 'text-[var(--bb-orange)]' : 'text-[var(--bb-smoke)]'}`}>{line}</span>
                ))}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};
