import React from 'react';
import type { PatchValidation, ValidationSummary } from '../types';

interface Props {
  validation?: ValidationSummary;
  patches?: PatchValidation[];
  warnings?: string[];
  qualityPassed?: boolean;
}

export const ValidationPanel: React.FC<Props> = ({
  validation,
  patches = [],
  warnings = [],
  qualityPassed = true,
}) => {
  if (!validation) {
    return (
      <div className="p-4 bg-[var(--bb-surface)] border border-[var(--bb-border)] text-center text-xs text-[var(--bb-smoke)] font-mono">
        NO CALIBRATION PROFILE DERIVED YET. INSPECT AND RUN DERIVE TO VIEW METRICS.
      </div>
    );
  }

  return (
    <div className="space-y-3 font-mono">
      {/* Top Status Header */}
      <div className="flex items-center justify-between p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
        <div className="flex items-center gap-2">
          <span
            className={`w-2.5 h-2.5 rounded-full ${
              qualityPassed ? 'bg-[var(--bb-gold)]' : 'bg-[var(--bb-crimson)]'
            }`}
          />
          <span className="font-bold text-[11px] tracking-wider text-[var(--bb-white)]">
            {qualityPassed ? 'CALIBRATION PASS' : 'CALIBRATION OVERRIDE / WARNING'}
          </span>
        </div>
        <span className="text-[10px] text-[var(--bb-smoke)]">CIEDE2000 D65</span>
      </div>

      {/* Primary Metrics Grid */}
      <div className="grid grid-cols-4 gap-1.5 text-center">
        <div className="p-2 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[9px] text-[var(--bb-smoke)] font-bold">MEAN ΔE</div>
          <div className="text-base font-bold text-[var(--bb-gold)]">
            {validation.meanDeltaE.toFixed(2)}
          </div>
        </div>
        <div className="p-2 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[9px] text-[var(--bb-smoke)] font-bold">95TH %-ILE</div>
          <div className="text-base font-bold text-[var(--bb-amber)]">
            {validation.p95DeltaE.toFixed(2)}
          </div>
        </div>
        <div className="p-2 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[9px] text-[var(--bb-smoke)] font-bold">MAX ΔE</div>
          <div className="text-base font-bold text-[var(--bb-orange)]">
            {validation.maxDeltaE.toFixed(2)}
          </div>
        </div>
        <div className="p-2 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[9px] text-[var(--bb-smoke)] font-bold">COND №</div>
          <div className="text-base font-bold text-[var(--bb-sand)]">
            {validation.conditionNumber.toFixed(2)}
          </div>
        </div>
      </div>

      {/* Warnings Banner */}
      {warnings.length > 0 && (
        <div className="p-2.5 bg-[var(--bb-ember-dark)]/40 border border-[var(--bb-crimson)] text-[11px] space-y-1">
          <div className="text-[var(--bb-gold)] font-bold flex items-center gap-1.5">
            <span>⚠</span> SPECTRAL ADVISORY
          </div>
          <ul className="list-disc list-inside text-[10px] text-[var(--bb-sand)] space-y-0.5">
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </div>
      )}

      {/* 24-Patch Comparative Table */}
      {patches.length > 0 && (
        <div className="space-y-1">
          <div className="text-[10px] font-bold text-[var(--bb-smoke)] tracking-wider flex justify-between">
            <span>24-PATCH SPECTRAL SAMPLING</span>
            <span>CORRECTED VS REF</span>
          </div>
          <div className="max-h-[180px] overflow-y-auto border border-[var(--bb-border)] bg-[var(--bb-vacuum)]">
            <table className="w-full text-left text-[10px] border-collapse">
              <thead className="bg-[var(--bb-panel)] text-[var(--bb-smoke)] sticky top-0 border-b border-[var(--bb-border)]">
                <tr>
                  <th className="p-1">#</th>
                  <th className="p-1">PATCH</th>
                  <th className="p-1">CORRECTED</th>
                  <th className="p-1">TARGET</th>
                  <th className="p-1 text-right">ΔE00</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--bb-border)]">
                {patches.map((p, idx) => {
                  const cRgb = `rgb(${Math.round(Math.min(1, Math.max(0, p.correctedRgb[0])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.correctedRgb[1])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.correctedRgb[2])) * 255)})`;
                  const tRgb = `rgb(${Math.round(Math.min(1, Math.max(0, p.targetRgb[0])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.targetRgb[1])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.targetRgb[2])) * 255)})`;
                  return (
                    <tr key={idx} className="hover:bg-[var(--bb-surface)]">
                      <td className="p-1 text-[var(--bb-ash)]">{idx + 1}</td>
                      <td className="p-1 text-[var(--bb-sand)] truncate max-w-[100px]">{p.patch}</td>
                      <td className="p-1">
                        <span className="inline-block w-4 h-3 border border-[var(--bb-border)]" style={{ backgroundColor: cRgb }} />
                      </td>
                      <td className="p-1">
                        <span className="inline-block w-4 h-3 border border-[var(--bb-border)]" style={{ backgroundColor: tRgb }} />
                      </td>
                      <td className="p-1 text-right font-bold text-[var(--bb-gold)]">
                        {p.deltaE.toFixed(2)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
};
