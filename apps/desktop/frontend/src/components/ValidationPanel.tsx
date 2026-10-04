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
      <div className="p-6 bg-[var(--bb-surface)] border border-[var(--bb-border)] text-center text-xs text-[var(--bb-smoke)]">
        NO CALIBRATION PROFILE DERIVED YET. INSPECT AND RUN DERIVE TO VIEW THERMAL DELTA-E DIAGNOSTICS.
      </div>
    );
  }

  return (
    <div className="space-y-4 p-4 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] rounded-sm">
      {/* Top Radiance Quality Badge */}
      <div className="flex items-center justify-between p-3 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
        <div className="flex items-center gap-2">
          <span
            className={`w-3 h-3 rounded-full ${
              qualityPassed ? 'bg-[var(--bb-gold)]' : 'bg-[var(--bb-crimson)]'
            }`}
          />
          <span className="font-bold text-xs tracking-wider text-[var(--bb-white)]">
            {qualityPassed ? 'CALIBRATION METRICS PASS' : 'CALIBRATION WARNING / OVERRIDE'}
          </span>
        </div>
        <span className="text-[10px] text-[var(--bb-smoke)]">
          CIEDE2000 STANDARD (D65 OBSERVER)
        </span>
      </div>

      {/* Primary Thermodynamic Metrics Grid */}
      <div className="grid grid-cols-4 gap-2">
        <div className="p-2.5 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[10px] text-[var(--bb-smoke)]">MEAN ΔE2000</div>
          <div className="text-lg font-bold text-[var(--bb-gold)]">
            {validation.meanDeltaE.toFixed(3)}
          </div>
          <div className="text-[9px] text-[var(--bb-ash)]">TARGET &lt; 2.0</div>
        </div>
        <div className="p-2.5 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[10px] text-[var(--bb-smoke)]">95TH %-ILE ΔE</div>
          <div className="text-lg font-bold text-[var(--bb-amber)]">
            {validation.p95DeltaE.toFixed(3)}
          </div>
          <div className="text-[9px] text-[var(--bb-ash)]">TARGET &lt; 3.5</div>
        </div>
        <div className="p-2.5 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[10px] text-[var(--bb-smoke)]">MAX ΔE2000</div>
          <div className="text-lg font-bold text-[var(--bb-orange)]">
            {validation.maxDeltaE.toFixed(3)}
          </div>
          <div className="text-[9px] text-[var(--bb-ash)]">PEAK SINGLE PATCH</div>
        </div>
        <div className="p-2.5 bg-[var(--bb-surface)] border border-[var(--bb-border)]">
          <div className="text-[10px] text-[var(--bb-smoke)]">MATRIX COND №</div>
          <div className="text-lg font-bold text-[var(--bb-sand)]">
            {validation.conditionNumber.toFixed(2)}
          </div>
          <div className="text-[9px] text-[var(--bb-ash)]">WELL-CONDITIONED &lt; 3</div>
        </div>
      </div>

      {/* Warnings / Caveats Banner */}
      {warnings.length > 0 && (
        <div className="p-3 bg-[var(--bb-ember-dark)]/40 border border-[var(--bb-crimson)] text-xs space-y-1">
          <div className="text-[var(--bb-gold)] font-bold flex items-center gap-1.5">
            <span>⚠</span> SPECTRAL QUALITY ADVISORY
          </div>
          <ul className="list-disc list-inside text-[11px] text-[var(--bb-sand)] space-y-0.5">
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </div>
      )}

      {/* 24-Patch Comparative Thermal Light-Box Table */}
      {patches.length > 0 && (
        <div className="space-y-1.5">
          <div className="text-[11px] font-semibold text-[var(--bb-smoke)] tracking-wide flex justify-between">
            <span>24-PATCH SPECTRAL MEASUREMENTS</span>
            <span>CORRECTED VS REFERENCE</span>
          </div>
          <div className="max-h-[220px] overflow-y-auto border border-[var(--bb-border)]">
            <table className="w-full text-left text-[11px] font-mono border-collapse">
              <thead className="bg-[var(--bb-panel)] text-[var(--bb-smoke)] sticky top-0 border-b border-[var(--bb-border)]">
                <tr>
                  <th className="p-1.5">#</th>
                  <th className="p-1.5">PATCH NAME</th>
                  <th className="p-1.5">CORRECTED</th>
                  <th className="p-1.5">TARGET</th>
                  <th className="p-1.5 text-right">ΔE2000</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--bb-border)]">
                {patches.map((p, idx) => {
                  const cRgb = `rgb(${Math.round(Math.min(1, Math.max(0, p.correctedRgb[0])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.correctedRgb[1])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.correctedRgb[2])) * 255)})`;
                  const tRgb = `rgb(${Math.round(Math.min(1, Math.max(0, p.targetRgb[0])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.targetRgb[1])) * 255)}, ${Math.round(Math.min(1, Math.max(0, p.targetRgb[2])) * 255)})`;
                  
                  return (
                    <tr key={idx} className="hover:bg-[var(--bb-surface)]">
                      <td className="p-1.5 text-[var(--bb-ash)]">{idx + 1}</td>
                      <td className="p-1.5 text-[var(--bb-sand)]">{p.patch}</td>
                      <td className="p-1.5">
                        <div className="flex items-center gap-2">
                          <span
                            className="inline-block w-4 h-3.5 border border-[var(--bb-border)]"
                            style={{ backgroundColor: cRgb }}
                          />
                        </div>
                      </td>
                      <td className="p-1.5">
                        <div className="flex items-center gap-2">
                          <span
                            className="inline-block w-4 h-3.5 border border-[var(--bb-border)]"
                            style={{ backgroundColor: tRgb }}
                          />
                        </div>
                      </td>
                      <td className="p-1.5 text-right font-bold text-[var(--bb-gold)]">
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
