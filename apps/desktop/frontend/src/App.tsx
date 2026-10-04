import React, { useEffect, useRef, useState } from 'react';
import type { ChartQuad, ChartRevision, DeriveResult, BatchSummary, InspectResult } from './types';
import { backend, chooseDirectory, chooseImage, chooseSavePath, listenForBatchProgress, listenForFileDrop, listenForFileDropHover } from './tauri';
import type { BatchProgress } from './tauri';
import { referenceFromDrop } from './interaction';
import { LightTableOverlay } from './components/LightTableOverlay';
import { ValidationPanel } from './components/ValidationPanel';
import {
  Layers,
  Sparkles,
  FolderOpen,
  Play,
  Cpu,
  CheckCircle2,
  AlertCircle,
  Flame,
  Upload,
  SearchCheck,
  X,
  Save,
} from 'lucide-react';

export const App: React.FC = () => {
  const [step, setStep] = useState<1 | 2 | 3 | 4>(1);

  // Workflow State
  const [referencePath, setReferencePath] = useState<string>('');
  const [referencePreview, setReferencePreview] = useState<string>('');
  const [chartRevision, setChartRevision] = useState<ChartRevision>('classic-before-nov-2014');
  const [quickAndDirty, setQuickAndDirty] = useState<boolean>(false);
  const [forceDerive] = useState<boolean>(false);

  // Chart Quadrilateral State
  const [quad, setQuad] = useState<ChartQuad>([
    { x: 40, y: 40 },
    { x: 440, y: 34 },
    { x: 440, y: 280 },
    { x: 40, y: 280 },
  ]);

  // Derived Calibration Profile State
  const [deriveResult, setDeriveResult] = useState<DeriveResult | null>(null);
  const [inspectResult, setInspectResult] = useState<InspectResult | null>(null);
  const [isProcessing, setIsProcessing] = useState<boolean>(false);
  const [isDropActive, setIsDropActive] = useState<boolean>(false);
  const [errorMessage, setErrorMessage] = useState<string>('');
  const referenceInputRef = useRef<HTMLInputElement>(null);
  // Batch Processing State
  const [batchInputPath, setBatchInputPath] = useState<string>('');
  const [batchOutputPath, setBatchOutputPath] = useState<string>('');
  const [overwriteOutputs, setOverwriteOutputs] = useState<boolean>(false);
  const [batchSummary, setBatchSummary] = useState<BatchSummary | null>(null);
  const [batchProgress, setBatchProgress] = useState<BatchProgress | null>(null);
  useEffect(() => {
    const preventDefault = (event: DragEvent) => event.preventDefault();
    const handleDrop = (event: DragEvent) => {
      event.preventDefault();
      setIsDropActive(false);
      const file = Array.from(event.dataTransfer?.files ?? []).find((item) => /\.(dng|jpe?g|png)$/i.test(item.name));
      if (!file) {
        setErrorMessage('Drop a supported DNG, JPEG, or PNG reference image.');
        return;
      }
      setReferencePath(file.name);
      setReferencePreview(URL.createObjectURL(file));
      setErrorMessage('Browser mode loaded the preview; use the native desktop app to process its filesystem path.');
      setStep(1);
    };
    window.addEventListener('dragover', preventDefault);
    window.addEventListener('drop', handleDrop);
    return () => {
      window.removeEventListener('dragover', preventDefault);
      window.removeEventListener('drop', handleDrop);
    };
  }, []);

  useEffect(() => {
    let stopDrop: (() => void) | undefined;
    let stopHover: (() => void) | undefined;
    listenForFileDrop(({ paths }) => {
      const supported = referenceFromDrop(paths);
      if (supported) {
        setReferencePath(supported);
        setErrorMessage('');
        setStep(1);
      }
    }).then((stop) => {
      stopDrop = stop;
    }).catch(() => {
      // Browser preview has no native event bus.
    });
    listenForFileDropHover(setIsDropActive).then((stop) => {
      stopHover = stop;
    }).catch(() => {
      // Browser preview has no native event bus.
    });
    return () => {
      stopDrop?.();
      stopHover?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listenForBatchProgress(setBatchProgress)
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => {
        // Browser preview has no Tauri event bus.
      });
    return () => unlisten?.();
  }, []);

  const browseReference = async () => {
    try {
      const selected = await chooseImage();
      if (selected) {
        setReferencePath(selected);
        setErrorMessage('');
      }
    } catch {
      referenceInputRef.current?.click();
    }
  };

  const handleBrowserReference = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    setReferencePath(file.name);
    setReferencePreview(URL.createObjectURL(file));
    setErrorMessage('Browser fallback selected the preview. Native processing requires the desktop file picker path.');
  };

  const inspectReference = async () => {
    if (!referencePath) {
      setErrorMessage('Choose or drop a reference image first.');
      return;
    }
    setIsProcessing(true);
    setErrorMessage('');
    try {
      const result = await backend.inspectReference(referencePath, chartRevision, quad, quickAndDirty);
      setInspectResult(result);
      setQuad(result.quad.map(([x, y]) => ({ x, y })) as ChartQuad);
    } catch (error: unknown) {
      setErrorMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setIsProcessing(false);
    }
  };

  // Simulated / Mock Demo Loader when running in pure browser environment
  const loadSyntheticDemo = () => {
    setReferencePath('20261003_183314.jpg');
    setReferencePreview('/test-data/20261003_183314.jpg');
    setQuickAndDirty(true);
    setDeriveResult({
      profilePath: 'studio_calibration.cbprofile.json',
      reportPath: 'studio_calibration_report.html',
      digest: '0396277212d10d4818b4f46510d9b587636ecedd6646af1de5770fe702cc019e',
      qualityPassed: true,
      warnings: quickAndDirty ? ['Non-RAW JPEG source: approximate sRGB linearization active.'] : [],
      validation: {
        meanDeltaE: 0.842,
        medianDeltaE: 0.691,
        p95DeltaE: 1.834,
        maxDeltaE: 2.145,
        neutralMaxDeltaE: 0.421,
        skinMaxDeltaE: 0.762,
        conditionNumber: 1.14,
        patchCount: 24,
      },
      patches: [
        { patch: 'DarkSkin', sourceRgb: [0.17, 0.08, 0.05], correctedRgb: [0.174, 0.079, 0.053], targetRgb: [0.174, 0.079, 0.053], deltaE: 0.42 },
        { patch: 'LightSkin', sourceRgb: [0.38, 0.25, 0.22], correctedRgb: [0.382, 0.248, 0.219], targetRgb: [0.385, 0.247, 0.218], deltaE: 0.68 },
        { patch: 'BlueSky', sourceRgb: [0.11, 0.18, 0.32], correctedRgb: [0.112, 0.179, 0.318], targetRgb: [0.114, 0.181, 0.315], deltaE: 0.81 },
        { patch: 'Foliage', sourceRgb: [0.13, 0.21, 0.09], correctedRgb: [0.131, 0.209, 0.089], targetRgb: [0.133, 0.208, 0.091], deltaE: 0.74 },
        { patch: 'White', sourceRgb: [0.88, 0.88, 0.83], correctedRgb: [0.879, 0.885, 0.834], targetRgb: [0.879, 0.885, 0.834], deltaE: 0.15 },
        { patch: 'Neutral8', sourceRgb: [0.58, 0.58, 0.56], correctedRgb: [0.582, 0.584, 0.562], targetRgb: [0.583, 0.584, 0.561], deltaE: 0.22 },
        { patch: 'Black', sourceRgb: [0.03, 0.03, 0.03], correctedRgb: [0.031, 0.031, 0.032], targetRgb: [0.031, 0.031, 0.032], deltaE: 0.09 },
      ],
    });
  };

  const handleRunDerive = () => {
    if (!referencePath) {
      setErrorMessage('Choose or drop a reference image before deriving a profile.');
      return;
    }
    setErrorMessage('');
    setIsProcessing(true);
    setTimeout(async () => {
      try {
        const result = await backend.deriveProfile(
          referencePath,
          chartRevision,
          'colorbalance_profile.cbprofile.json',
          'colorbalance_report.html',
          quad,
          quickAndDirty,
          forceDerive,
        );
        setDeriveResult(result);
        setStep(2);
      } catch (error: unknown) {
        setErrorMessage(error instanceof Error ? error.message : String(error));
      } finally {
        setIsProcessing(false);
      }
    }, 50);
  };

  const handleRunBatch = () => {
    if (!batchInputPath || !batchOutputPath || !deriveResult?.profilePath) {
      setErrorMessage('Select source and destination folders after deriving a profile.');
      return;
    }
    setErrorMessage('');
    setIsProcessing(true);
    setTimeout(async () => {
      try {
        const result = await backend.applyBatch(
          deriveResult.profilePath,
          batchInputPath,
          batchOutputPath,
          overwriteOutputs,
          false,
        );
        setBatchSummary(result);
        setStep(4);
      } catch (error: unknown) {
        setErrorMessage(error instanceof Error ? error.message : String(error));
      } finally {
        setIsProcessing(false);
      }
    }, 50);
  };

  return (
    <div className="w-screen h-screen flex flex-col bg-[var(--bb-space)] text-[var(--bb-sand)] font-mono select-none overflow-hidden">
      {/* Top Futuristic Masthead / HUD */}
      <header className="h-12 border-b border-[var(--bb-border)] bg-[var(--bb-vacuum)] px-4 flex items-center justify-between text-xs">
        <div className="flex items-center gap-3">
          <div className="w-5 h-5 rounded-xs bg-[var(--bb-ember)] flex items-center justify-center border border-[var(--bb-gold)] thermal-glow">
            <Flame className="w-3.5 h-3.5 text-[var(--bb-incandescent)]" />
          </div>
          <span className="font-bold tracking-widest text-[var(--bb-gold)] text-sm">
            COLORBALANCE // LIGHT-TABLE OS
          </span>
          <span className="px-2 py-0.5 text-[10px] bg-[var(--bb-panel)] text-[var(--bb-amber)] border border-[var(--bb-border)]">
            PLANCK ENGINE v0.1.0
          </span>
        </div>

        {/* 4-Step Navigation Buttons */}
        <nav className="flex items-center gap-1">
          {[
            { id: 1, label: '01.REFERENCE', icon: Layers },
            { id: 2, label: '02.VALIDATE', icon: Sparkles },
            { id: 3, label: '03.BATCH QUEUE', icon: FolderOpen },
            { id: 4, label: '04.PROCESS', icon: Cpu },
          ].map((item) => {
            const Icon = item.icon;
            const isActive = step === item.id;
            return (
              <button
                key={item.id}
                onClick={() => setStep(item.id as 1 | 2 | 3 | 4)}
                className={`px-3 py-1.5 flex items-center gap-2 text-xs font-semibold tracking-wider transition-all border ${
                  isActive
                    ? 'bg-[var(--bb-surface)] text-[var(--bb-gold)] border-[var(--bb-gold)] shadow-[0_0_10px_rgba(250,151,35,0.2)]'
                    : 'text-[var(--bb-smoke)] border-transparent hover:text-[var(--bb-sand)] hover:bg-[var(--bb-panel)]'
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                {item.label}
              </button>
            );
          })}
        </nav>

        {/* Engine Status Diagnostic */}
        <div className="flex items-center gap-3 text-[11px] text-[var(--bb-ash)]">
          <span className="flex items-center gap-1 text-[var(--bb-amber)]">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--bb-gold)] animate-pulse"></span>
            ENGINE READY
          </span>
          <span>SRGB-D65</span>
        </div>
      </header>


      {errorMessage && (
        <div className="absolute top-14 left-1/2 -translate-x-1/2 z-[60] max-w-[760px] px-4 py-3 bg-[var(--bb-ember-dark)] border border-[var(--bb-crimson)] text-[11px] text-[var(--bb-white)] shadow-[0_0_32px_rgba(196,39,12,0.45)] flex items-center gap-3">
          <AlertCircle className="w-4 h-4 text-[var(--bb-orange)] shrink-0" />
          <span className="flex-1">{errorMessage}</span>
          <button type="button" aria-label="Dismiss error" onClick={() => setErrorMessage('')} className="p-1 text-[var(--bb-smoke)] hover:text-[var(--bb-white)]">
            <X className="w-4 h-4" />
          </button>
        </div>
      )}
      {/* Main Workspace Area */}
      <main className={`flex-1 flex overflow-hidden relative ${isDropActive ? 'ring-2 ring-inset ring-[var(--bb-gold)]' : ''}`}>
        {isDropActive && (
          <div className="absolute inset-4 z-50 border-2 border-dashed border-[var(--bb-gold)] bg-[var(--bb-space)]/90 flex items-center justify-center pointer-events-none thermal-glow">
            <div className="text-center">
              <Upload className="w-10 h-10 mx-auto text-[var(--bb-gold)] mb-3" />
              <div className="text-sm font-bold tracking-[0.25em] text-[var(--bb-white)]">DROP REFERENCE FRAME</div>
              <div className="text-[10px] text-[var(--bb-amber)] mt-2">DNG · JPEG · PNG / LOCAL PROCESSING ONLY</div>
            </div>
          </div>
        )}
        <div className="flex-1 h-full border-r border-[var(--bb-border)] flex flex-col bg-[var(--bb-vacuum)]">
          <LightTableOverlay
            imageSrc={referencePreview}
            quad={quad}
            onQuadChange={setQuad}
            onBrowse={browseReference}
            disabled={isProcessing}
          />
        </div>

        {/* Right Side: Step-specific Control HUD */}
        <aside className="w-[440px] h-full bg-[var(--bb-surface)] p-4 flex flex-col justify-between overflow-y-auto border-l border-[var(--bb-border)]">
          {step === 1 && (
            <div className="space-y-4">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-xs font-bold text-[var(--bb-gold)] tracking-wider flex items-center gap-2">
                  <Layers className="w-4 h-4 text-[var(--bb-amber)]" />
                  REFERENCE FRAME CALIBRATION
                </h2>
                <p className="text-[11px] text-[var(--bb-smoke)] mt-0.5">
                  Select a RAW (DNG) or compressed JPEG reference frame containing a 24-patch ColorChecker Classic.
                </p>
              </div>

              <input
                ref={referenceInputRef}
                type="file"
                accept=".dng,.jpg,.jpeg,.png,image/jpeg,image/png"
                onChange={handleBrowserReference}
                className="hidden"
                aria-hidden="true"
                tabIndex={-1}
              />
              <div className="space-y-2">
                <label className="text-[10px] text-[var(--bb-smoke)] font-bold">SOURCE FILE PATH</label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={referencePath}
                    onChange={(e) => setReferencePath(e.target.value)}
                    placeholder="DROP IMAGE HERE OR BROWSE…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-3 py-2 text-xs text-[var(--bb-sand)] focus:border-[var(--bb-gold)] focus:shadow-[0_0_12px_rgba(252,194,56,0.15)] outline-none transition-all"
                  />
                  <button type="button" onClick={browseReference} className="ui-button ui-button-secondary">
                    <FolderOpen className="w-3.5 h-3.5" /> BROWSE
                  </button>
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <button type="button" onClick={inspectReference} disabled={isProcessing || !referencePath} className="ui-button ui-button-secondary disabled:opacity-40">
                    <SearchCheck className="w-3.5 h-3.5" /> {isProcessing ? 'SCANNING…' : 'INSPECT CHART'}
                  </button>
                  <button type="button" onClick={loadSyntheticDemo} className="ui-button ui-button-ghost">
                    <Sparkles className="w-3.5 h-3.5" /> LOAD UI DEMO
                  </button>
                </div>
                {inspectResult && (
                  <div className={`px-3 py-2 border text-[10px] ${inspectResult.qualityPassed ? 'border-[var(--bb-amber)] text-[var(--bb-gold)] bg-[var(--bb-panel)]' : 'border-[var(--bb-crimson)] text-[var(--bb-orange)] bg-[var(--bb-ember-dark)]/40'}`}>
                    {inspectResult.qualityPassed ? '✓ CHART PASSED QUALITY GATES' : `⚠ ${inspectResult.gateFailures.length} QUALITY GATE WARNING(S)`}
                  </div>
                )}
              </div>

              {/* Physical Chart Revision Selection */}
              <div className="space-y-1.5">
                <label className="text-[10px] text-[var(--bb-smoke)] font-bold">PHYSICAL CHART REVISION</label>
                <select
                  value={chartRevision}
                  onChange={(e) => setChartRevision(e.target.value as ChartRevision)}
                  className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2.5 py-1.5 text-xs text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                >
                  <option value="classic-before-nov-2014">ColorChecker Classic (Pre-Nov 2014)</option>
                  <option value="classic-from-nov-2014">ColorChecker Classic / Calibrite (Post-Nov 2014)</option>
                </select>
              </div>

              {/* Quick & Dirty Mode Toggle */}
              <div className="p-3 bg-[var(--bb-panel)] border border-[var(--bb-border)] space-y-2">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={quickAndDirty}
                    onChange={(e) => setQuickAndDirty(e.target.checked)}
                    className="accent-[var(--bb-amber)]"
                  />
                  <span className="text-xs font-bold text-[var(--bb-gold)]">
                    QUICK & DIRTY APPROXIMATION
                  </span>
                </label>
                <p className="text-[10px] text-[var(--bb-smoke)] leading-relaxed">
                  Inverts sRGB gamma non-linearities on non-RAW JPEG/PNG sources and relaxes neutral row gates for fast field calibration.
                </p>
              </div>

              {/* Action Buttons */}
              <div className="pt-2">
                <button
                  type="button"
                  onClick={handleRunDerive}
                  disabled={isProcessing || !referencePath}
                  className="ui-button ui-button-primary w-full py-3 disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  <Play className="w-4 h-4 fill-current" />
                  {isProcessing ? 'CALCULATING 3×3 FIT…' : 'DERIVE COLOR PROFILE'}
                </button>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="space-y-4">
              <div className="border-b border-[var(--bb-border)] pb-2 flex justify-between items-center">
                <div>
                  <h2 className="text-xs font-bold text-[var(--bb-gold)] tracking-wider">
                    CALIBRATION VALIDATION
                  </h2>
                  <p className="text-[11px] text-[var(--bb-smoke)]">
                    Inspect patch error distributions and matrix condition numbers.
                  </p>
                </div>
              </div>

              <ValidationPanel
                validation={deriveResult?.validation}
                patches={deriveResult?.patches}
                warnings={deriveResult?.warnings}
                qualityPassed={deriveResult?.qualityPassed}
              />

              <div className="pt-2 flex gap-2">
                <button
                  type="button"
                  onClick={() => setStep(3)}
                  disabled={!deriveResult}
                  className="ui-button ui-button-primary flex-1 py-2.5 disabled:opacity-40"
                >
                  CONTINUE TO BATCH QUEUE →
                </button>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="space-y-4">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-xs font-bold text-[var(--bb-gold)] tracking-wider flex items-center gap-2">
                  <FolderOpen className="w-4 h-4 text-[var(--bb-amber)]" />
                  BATCH PROCESSING DIRECTORY
                </h2>
                <p className="text-[11px] text-[var(--bb-smoke)]">
                  Select matching image folders for batch calibration and 16-bit TIFF export.
                </p>
              </div>

              {/* Input Batch Folder */}
              <div className="space-y-1.5">
                <label className="text-[10px] text-[var(--bb-smoke)] font-bold">SOURCE IMAGE FOLDER</label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={batchInputPath}
                    onChange={(e) => setBatchInputPath(e.target.value)}
                    placeholder="SELECT MATCHING IMAGE SERIES…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-3 py-2 text-xs text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                  />
                  <button type="button" className="ui-button ui-button-secondary" onClick={async () => {
                    try { const path = await chooseDirectory(); if (path) setBatchInputPath(path); }
                    catch { setErrorMessage('Directory picker requires the desktop application.'); }
                  }}><FolderOpen className="w-3.5 h-3.5" /> SOURCE</button>
                </div>
              </div>

              <div className="space-y-1.5">
                <label className="text-[10px] text-[var(--bb-smoke)] font-bold">DESTINATION FOLDER</label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={batchOutputPath}
                    onChange={(e) => setBatchOutputPath(e.target.value)}
                    placeholder="SELECT OUTPUT DESTINATION…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-3 py-2 text-xs text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                  />
                  <button type="button" className="ui-button ui-button-secondary" onClick={async () => {
                    try { const path = await chooseDirectory(); if (path) setBatchOutputPath(path); }
                    catch { setErrorMessage('Directory picker requires the desktop application.'); }
                  }}><FolderOpen className="w-3.5 h-3.5" /> OUTPUT</button>
                </div>
              </div>

              {/* Overwrite Safety Policy */}
              <div className="p-3 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={overwriteOutputs}
                    onChange={(e) => setOverwriteOutputs(e.target.checked)}
                    className="accent-[var(--bb-amber)]"
                  />
                  <span className="text-xs font-bold text-[var(--bb-sand)]">
                    OVERWRITE EXISTING OUTPUTS
                  </span>
                </label>
                <p className="text-[10px] text-[var(--bb-smoke)] mt-1">
                  Default: Skips existing files to prevent unintended data loss.
                </p>
              </div>

              {isProcessing && batchProgress && (
                <div className="space-y-2 p-3 bg-[var(--bb-vacuum)] border border-[var(--bb-border)]">
                  <div className="flex justify-between text-[10px] text-[var(--bb-smoke)]">
                    <span className="truncate max-w-[280px]">{batchProgress.file ?? 'FINALIZING OUTPUTS…'}</span>
                    <span>{batchProgress.completed}/{batchProgress.total}</span>
                  </div>
                  <div className="h-1.5 bg-[var(--bb-charcoal)] border border-[var(--bb-border)] overflow-hidden">
                    <div className="h-full bg-gradient-to-r from-[var(--bb-crimson)] via-[var(--bb-orange)] to-[var(--bb-gold)] transition-all" style={{ width: `${batchProgress.total > 0 ? (batchProgress.completed / batchProgress.total) * 100 : 0}%` }} />
                  </div>
                </div>
              )}
              <div className="grid grid-cols-4 gap-2">
                <button
                  type="button"
                  onClick={handleRunBatch}
                  disabled={isProcessing || !batchInputPath || !batchOutputPath || !deriveResult}
                  className="ui-button ui-button-primary col-span-3 py-3 disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  <Cpu className="w-4 h-4" />
                  {isProcessing ? 'PROCESSING BATCH…' : 'START BATCH APPLICATION'}
                </button>
                <button type="button" disabled={!isProcessing} onClick={async () => { try { await backend.cancelBatch(); } catch (error: unknown) { setErrorMessage(error instanceof Error ? error.message : String(error)); } }} className="ui-button ui-button-ghost disabled:opacity-30">
                  <X className="w-3.5 h-3.5" /> STOP
                </button>
              </div>
            </div>
          )}

          {step === 4 && (
            <div className="space-y-4">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-xs font-bold text-[var(--bb-gold)] tracking-wider flex items-center gap-2">
                  <CheckCircle2 className="w-4 h-4 text-[var(--bb-gold)]" />
                  BATCH PROCESSING COMPLETE
                </h2>
                <p className="text-[11px] text-[var(--bb-smoke)]">
                  Summary of transformed 16-bit linear sRGB TIFF outputs.
                </p>
              </div>

              {batchSummary && (
                <div className="space-y-3">
                  <div className="grid grid-cols-3 gap-2">
                    <div className="p-2 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                      <div className="text-[10px] text-[var(--bb-smoke)]">PROCESSED</div>
                      <div className="text-base font-bold text-[var(--bb-gold)]">
                        {batchSummary.succeeded.length}
                      </div>
                    </div>
                    <div className="p-2 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                      <div className="text-[10px] text-[var(--bb-smoke)]">SKIPPED</div>
                      <div className="text-base font-bold text-[var(--bb-amber)]">
                        {batchSummary.skipped.length}
                      </div>
                    </div>
                    <div className="p-2 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                      <div className="text-[10px] text-[var(--bb-smoke)]">FAILED</div>
                      <div className="text-base font-bold text-[var(--bb-crimson)]">
                        {batchSummary.failed.length}
                      </div>
                    </div>
                  </div>

                  {/* Export Options */}
                  <div className="p-3 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] space-y-2">
                    <span className="text-[10px] font-bold text-[var(--bb-smoke)]">INTERCHANGE TRANSFORMS</span>
                    <div className="grid grid-cols-2 gap-2">
                      {(['clf', 'cube'] as const).map((format) => (
                        <button key={format} type="button" className="ui-button ui-button-secondary" onClick={async () => {
                          if (!deriveResult?.profilePath) { setErrorMessage('Derive a profile before exporting.'); return; }
                          try {
                            const output = await chooseSavePath(`colorbalance.${format}`, format);
                            if (output) await backend.exportProfile(deriveResult.profilePath, format, output, 33);
                          } catch (error: unknown) {
                            setErrorMessage(error instanceof Error ? error.message : String(error));
                          }
                        }}>
                          <Save className="w-3.5 h-3.5" /> EXPORT .{format.toUpperCase()}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Footer Diagnostic Monospace Stamp */}
          <div className="pt-4 border-t border-[var(--bb-border)] text-[9px] text-[var(--bb-ash)] flex justify-between items-center">
            <span>TAURI LIGHT-TABLE // DESKTOP OS</span>
            <span>4-POINT WARP // CIEDE2000</span>
          </div>
        </aside>
      </main>
    </div>
  );
};

export default App;
