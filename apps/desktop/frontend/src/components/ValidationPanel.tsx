import React from 'react';
import type { InspectGateFailure, PatchValidation, ValidationSummary } from '../types';
import { QualityFailures } from './QualityFailures';

interface Props {
  validation?: ValidationSummary;
  patches?: PatchValidation[];
  warnings?: string[];
  qualityPassed?: boolean;
  gateFailures?: InspectGateFailure[];
}

export const ValidationPanel: React.FC<Props> = ({
  validation,
  patches = [],
  warnings = [],
  qualityPassed = true,
  gateFailures = [],
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
          <span className="font-bold text-[11px] tracking-wider text-[var(--bb-white)]" data-testid="quality-badge">
            {qualityPassed ? 'CAPTURE QUALITY PASS' : `PROCESSED WITH ${Math.max(warnings.length, 1)} WARNING(S)`}
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


      {/* Weakest fit patches */}
      {patches.length > 0 && (
        <div className="space-y-1">
          <div className="text-[10px] font-bold text-[var(--bb-smoke)] tracking-wider">WEAKEST PATCHES (FIT ΔE00)</div>
          <ol className="grid grid-cols-2 gap-x-3 text-[10px] text-[var(--bb-sand)]" data-testid="weakest-patches">
            {[...patches]
              .sort((a, b) => b.deltaE - a.deltaE)
              .slice(0, 5)
              .map((p) => (
                <li key={p.patch} className="flex justify-between">
                  <span>{p.patch}</span>
                  <span className="font-bold text-[var(--bb-orange)]">{p.deltaE.toFixed(2)}</span>
                </li>
              ))}
          </ol>
        </div>
      )}

      {/* Compact warnings: short causes up front, the unabridged failures one click away */}
      {(warnings.length > 0 || gateFailures.length > 0) && (
        <div className="p-2 bg-[var(--bb-ember-dark)]/40 border border-[var(--bb-crimson)] space-y-1.5" data-testid="warnings">
          <ul className="space-y-0.5 text-[10px] text-[var(--bb-sand)]">
            {warnings.map((w, i) => (
              <li key={i} className="flex gap-1.5">
                <span className="text-[var(--bb-gold)]">⚠</span>
                <span>{w}</span>
              </li>
            ))}
          </ul>
          {gateFailures.length > 0 && (
            <details>
              <summary className="cursor-pointer text-[9px] text-[var(--bb-smoke)] hover:text-[var(--bb-gold)]">
                DETAILS ({gateFailures.length} checks)
              </summary>
              <div className="pt-1.5">
                <QualityFailures failures={gateFailures} />
              </div>
            </details>
          )}
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
