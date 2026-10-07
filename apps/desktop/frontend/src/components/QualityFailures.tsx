import React from 'react';
import type { InspectGateFailure } from '../types';

interface Props {
  failures: InspectGateFailure[];
  /** Rows shown before the "show all" toggle. */
  previewCount?: number;
}

type Category = 'clipped' | 'noisy' | 'orientation' | 'other';

const categorize = (failure: InspectGateFailure): Category => {
  if (failure.reason.startsWith('clipped')) return 'clipped';
  if (failure.reason.includes('coefficient of variation')) return 'noisy';
  if (failure.reason.startsWith('last row is not the neutral row')) return 'orientation';
  return 'other';
};

const CATEGORY_LABEL: Record<Category, string> = {
  clipped: 'CLIPPED',
  noisy: 'NOISY',
  orientation: 'ORIENTATION',
  other: 'OTHER',
};

/**
 * Chart-quality gate failures grouped by kind, with every measured value
 * available. The backend supplies the failures; nothing is computed here.
 */
export const QualityFailures: React.FC<Props> = ({ failures, previewCount = 6 }) => {
  const [expanded, setExpanded] = React.useState(false);
  if (failures.length === 0) return null;

  const counts: Record<Category, number> = { clipped: 0, noisy: 0, orientation: 0, other: 0 };
  for (const failure of failures) counts[categorize(failure)] += 1;
  const visible = expanded ? failures : failures.slice(0, previewCount);

  return (
    <div className="space-y-1.5 font-mono" data-testid="quality-failures">
      <div className="flex flex-wrap gap-1">
        {(Object.keys(counts) as Category[])
          .filter((category) => counts[category] > 0)
          .map((category) => (
            <span
              key={category}
              className="px-1.5 py-0.5 border border-[var(--bb-crimson)] text-[9px] text-[var(--bb-orange)]"
            >
              {CATEGORY_LABEL[category]} ×{counts[category]}
            </span>
          ))}
      </div>
      <ul className="space-y-0.5 text-[9px] text-[var(--bb-sand)] max-h-40 overflow-y-auto">
        {visible.map((failure, index) => (
          <li key={`${failure.patch ?? 'chart'}-${failure.reason}-${index}`} className="flex justify-between gap-2">
            <span>
              <span className="text-[var(--bb-gold)]">{failure.patch ?? 'chart'}</span> {failure.reason}
            </span>
            <span className="text-[var(--bb-smoke)] shrink-0">{failure.measured}</span>
          </li>
        ))}
      </ul>
      {failures.length > previewCount && (
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="text-[9px] text-[var(--bb-amber)] underline"
        >
          {expanded ? 'SHOW FEWER' : `SHOW ALL ${failures.length}`}
        </button>
      )}
    </div>
  );
};
