import React from 'react';
import { cssColor } from '../interaction';
import type { PatchValidation } from '../types';

interface Props {
  patches: PatchValidation[];
  selected: string | null;
  onSelect: (patch: string | null) => void;
}

const COLUMNS = 6;

/** Split a CamelCase patch name for display: `DarkSkin` becomes `Dark Skin`. */
const label = (name: string) => name.replace(/([a-z])([A-Z0-9])/g, '$1 $2');

/**
 * The 24 chart patches in chart reading order (6 columns by 4 rows). Each cell
 * shows the corrected color on top of the dataset target. The bar under the
 * number is the patch's ΔE00 relative to the worst patch of this fit. It is a
 * ranking aid, not a pass/fail band: the quality gates live in the profile.
 */
export const PatchGrid: React.FC<Props> = ({ patches, selected, onSelect }) => {
  const worst = Math.max(...patches.map((p) => p.deltaE), Number.EPSILON);
  return (
    <div className="w-full h-full flex flex-col min-h-0 p-4 gap-3" data-testid="patch-grid">
      <div className="flex items-baseline justify-between text-[10px] text-[var(--bb-smoke)]">
        <span className="font-bold tracking-wider">FIT RESULT PER CHART PATCH</span>
        <span>top: corrected · bottom: chart target · bar: ΔE00 relative to the worst patch</span>
      </div>
      <div
        className="flex-1 min-h-0 grid gap-2"
        style={{
          gridTemplateColumns: `repeat(${COLUMNS}, minmax(0, 1fr))`,
          gridTemplateRows: `repeat(${Math.ceil(patches.length / COLUMNS)}, minmax(0, 1fr))`,
        }}
      >
        {patches.map((p) => {
          const isSelected = selected === p.patch;
          return (
            <button
              key={p.patch}
              type="button"
              onClick={() => onSelect(isSelected ? null : p.patch)}
              aria-pressed={isSelected}
              title={`${label(p.patch)}: ΔE00 ${p.deltaE.toFixed(2)}`}
              data-testid={`patch-cell-${p.patch}`}
              className={`relative min-h-0 flex flex-col border text-left overflow-hidden ${
                isSelected ? 'border-[var(--bb-gold)]' : 'border-[var(--bb-border)] hover:border-[var(--bb-border-bright)]'
              }`}
            >
              <span className="flex-1 min-h-0" style={{ background: cssColor(p.correctedSrgb) }} />
              <span className="flex-1 min-h-0" style={{ background: cssColor(p.targetSrgb) }} />
              <span className="absolute inset-x-0 bottom-0 px-1.5 py-1 bg-[var(--bb-vacuum)]/85 text-[10px] leading-tight">
                <span className="flex justify-between gap-1">
                  <span className="truncate text-[var(--bb-sand)]">{label(p.patch)}</span>
                  <span className="shrink-0 font-bold tabular-nums text-[var(--bb-white)]">{p.deltaE.toFixed(2)}</span>
                </span>
                <span className="block h-0.5 mt-0.5 bg-[var(--bb-border)]">
                  <span
                    className="block h-full bg-[var(--bb-orange)]"
                    style={{ width: `${(p.deltaE / worst) * 100}%` }}
                  />
                </span>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
};
