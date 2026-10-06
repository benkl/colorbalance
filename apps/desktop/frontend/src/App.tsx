import React, { useEffect, useRef, useState } from 'react';
import type { ChartQuad, ChartRevision, DeriveResult, BatchSummary, InspectResult } from './types';
import { backend, chooseDirectory, chooseImage, chooseSavePath, listenForBatchProgress, listenForFileDrop, listenForFileDropHover } from './tauri';
import type { BatchProgress } from './tauri';
import { referenceFromDrop } from './interaction';
import { LightTableOverlay } from './components/LightTableOverlay';
import { ValidationPanel } from './components/ValidationPanel';
import { DiagnosticConsole } from './components/DiagnosticConsole';
import { logger } from './logger';
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
  const [chartRevision, setChartRevision] = useState<ChartRevision | ''>('');
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
        logger.warn('UI', 'HTML drop rejected: unsupported format');
        setErrorMessage('Drop a supported DNG, JPEG, or PNG reference image.');
        return;
      }
      logger.info('UI', `HTML drop received: ${file.name}`);
      setReferencePath(file.name);
      setReferencePreview(URL.createObjectURL(file));
      setErrorMessage('');
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
      logger.info('UI', `Native window drop event: ${paths.join(', ')}`);
      const supported = referenceFromDrop(paths);
      if (supported) {
        logger.success('UI', `Selected reference from native drop: ${supported}`);
        setReferencePath(supported);
        setErrorMessage('');
        setStep(1);
      } else {
        logger.warn('UI', 'Native drop ignored: no supported RAW/JPEG/PNG found');
      }
    }).then((stop) => {
      stopDrop = stop;
    }).catch(() => {
      // Browser preview mode
    });
    listenForFileDropHover(setIsDropActive).then((stop) => {
      stopHover = stop;
    }).catch(() => {
      // Browser preview mode
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
        // Browser preview mode
      });
    return () => unlisten?.();
  }, []);

  const browseReference = async () => {
    logger.info('UI', 'Action: Browse reference frame');
    try {
      const selected = await chooseImage();
      if (selected) {
        logger.success('UI', `Reference selected via native picker: "${selected}"`);
        setReferencePath(selected);
        setErrorMessage('');
      }
    } catch (err: unknown) {
      logger.warn('UI', `Native dialog unavailable (${err instanceof Error ? err.message : String(err)}); falling back to HTML file input`);
      referenceInputRef.current?.click();
    }
  };

  const handleBrowserReference = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    logger.info('UI', `HTML file input selected: ${file.name}`);
    setReferencePath(file.name);
    setReferencePreview(URL.createObjectURL(file));
    setErrorMessage('');
  };

  const inspectReference = async () => {
    if (!referencePath) {
      setErrorMessage('Choose or drop a reference image first.');
      return;
    }
    if (!chartRevision) {
      setErrorMessage('Explicitly select the physical chart revision before inspecting.');
      return;
    }
    setIsProcessing(true);
    setErrorMessage('');
    try {
      const result = await backend.inspectReference(referencePath, chartRevision, quad, quickAndDirty);
      setInspectResult(result);
      setQuad(result.quad.map(([x, y]) => ({ x, y })) as ChartQuad);
      if (result.previewDataUrl) {
        setReferencePreview(result.previewDataUrl);
      }
    } catch (error: unknown) {
      setErrorMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setIsProcessing(false);
    }
  };

  const loadSyntheticDemo = () => {
    setReferencePath('20261003_183314.jpg');
    setReferencePreview('/test-data/20261003_183314.jpg');
    setChartRevision('classic-from-nov-2014');
    setQuickAndDirty(true);
    // Demo image is 1864×1398 — set quad to that coordinate space with 8% margins
    setQuad([
      { x: 149, y: 112 },   // 8% of 1864, 8% of 1398
      { x: 1715, y: 112 },  // 92%
      { x: 1715, y: 1286 }, // 92%
      { x: 149, y: 1286 },
    ]);
    setDeriveResult({
      profilePath: 'studio_calibration.cbprofile.json',
      reportPath: 'studio_calibration_report.html',
      digest: '0396277212d10d4818b4f46510d9b587636ecedd6646af1de5770fe702cc019e',
      qualityPassed: true,
      warnings: ['Non-RAW JPEG source: approximate sRGB linearization active.'],
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
    if (!chartRevision) {
      setErrorMessage('Explicitly select the physical chart revision before deriving a profile.');
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
        if (typeof window !== 'undefined' && !('__TAURI__' in window)) {
          logger.warn('UI', 'Browser preview: loaded synthetic demo calibration');
          loadSyntheticDemo();
          setStep(2);
        } else {
          setErrorMessage(error instanceof Error ? error.message : String(error));
        }
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
      {/* Top Precision Masthead */}
      <header className="h-10 shrink-0 border-b border-[var(--bb-border)] bg-[var(--bb-vacuum)] px-3 flex items-center justify-between text-xs">
        <div className="flex items-center gap-2.5">
          <div className="w-4 h-4 rounded-xs bg-[var(--bb-ember)] flex items-center justify-center border border-[var(--bb-gold)] thermal-glow">
            <Flame className="w-3 h-3 text-[var(--bb-incandescent)]" />
          </div>
          <span className="font-bold tracking-widest text-[var(--bb-gold)] text-xs">
            COLORBALANCE // LIGHT-TABLE OS
          </span>
          <span className="px-1.5 py-0.2 text-[9px] bg-[var(--bb-panel)] text-[var(--bb-amber)] border border-[var(--bb-border)]">
            v0.1.0
          </span>
        </div>

        {/* 4-Step Navigation Tabs */}
        <nav className="flex items-center gap-1">
          {[
            { id: 1, label: '01.REFERENCE', icon: Layers },
            { id: 2, label: '02.VALIDATE', icon: Sparkles },
            { id: 3, label: '03.BATCH', icon: FolderOpen },
            { id: 4, label: '04.PROCESS', icon: Cpu },
          ].map((item) => {
            const Icon = item.icon;
            const isActive = step === item.id;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setStep(item.id as 1 | 2 | 3 | 4)}
                className={`h-7 px-2.5 flex items-center gap-1.5 text-[10px] font-bold tracking-wider border cursor-pointer transition-all ${
                  isActive
                    ? 'bg-[var(--bb-surface)] text-[var(--bb-gold)] border-[var(--bb-gold)] shadow-[0_0_8px_rgba(245,185,49,0.2)]'
                    : 'text-[var(--bb-smoke)] border-transparent hover:text-[var(--bb-sand)] hover:bg-[var(--bb-panel)]'
                }`}
              >
                <Icon className="w-3 h-3" />
                {item.label}
              </button>
            );
          })}
        </nav>

        {/* Engine Status Diagnostic */}
        <div className="flex items-center gap-2 text-[10px] text-[var(--bb-ash)]">
          <span className="flex items-center gap-1 text-[var(--bb-amber)] font-bold">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--bb-gold)] animate-pulse" />
            ENGINE READY
          </span>
        </div>
      </header>

      {/* Global Error Banner */}
      {errorMessage && (
        <div className="shrink-0 px-3 py-1.5 bg-[var(--bb-ember-dark)] border-b border-[var(--bb-crimson)] text-[11px] text-[var(--bb-white)] flex items-center justify-between gap-2 shadow-[0_0_15px_rgba(179,35,11,0.5)]">
          <div className="flex items-center gap-2 min-w-0">
            <AlertCircle className="w-3.5 h-3.5 text-[var(--bb-orange)] shrink-0" />
            <span className="truncate">{errorMessage}</span>
          </div>
          <button
            type="button"
            aria-label="Dismiss error"
            onClick={() => setErrorMessage('')}
            className="p-0.5 text-[var(--bb-smoke)] hover:text-[var(--bb-white)] cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}

      {/* Main Grid: Left Viewport (1fr) + Right Inspector (400px Fixed) */}
      <main className={`flex-1 min-h-0 flex relative overflow-hidden ${isDropActive ? 'ring-2 ring-inset ring-[var(--bb-gold)]' : ''}`}>
        {/* Drop Zone Visual Indicator */}
        {isDropActive && (
          <div className="absolute inset-2 z-50 border-2 border-dashed border-[var(--bb-gold)] bg-[var(--bb-space)]/90 flex items-center justify-center pointer-events-none thermal-glow">
            <div className="text-center">
              <Upload className="w-8 h-8 mx-auto text-[var(--bb-gold)] mb-2" />
              <div className="text-xs font-bold tracking-[0.2em] text-[var(--bb-white)]">DROP REFERENCE FRAME</div>
              <div className="text-[9px] text-[var(--bb-amber)] mt-1">DNG · JPEG · PNG / LOCAL ONLY</div>
            </div>
          </div>
        )}

        {/* Left Side: Fully Contained Light-Table Viewport */}
        <div className="flex-1 min-w-0 h-full border-r border-[var(--bb-border)] flex flex-col bg-[var(--bb-vacuum)] overflow-hidden">
          <LightTableOverlay
            imageSrc={referencePreview}
            quad={quad}
            onQuadChange={setQuad}
            onBrowse={browseReference}
            disabled={isProcessing}
          />
        </div>

        {/* Right Side: Fixed Inspector HUD */}
        <aside className="w-[380px] shrink-0 h-full bg-[var(--bb-surface)] p-3.5 flex flex-col justify-between overflow-y-auto border-l border-[var(--bb-border)]">
          {step === 1 && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-[11px] font-bold text-[var(--bb-gold)] tracking-wider flex items-center gap-1.5">
                  <Layers className="w-3.5 h-3.5 text-[var(--bb-amber)]" />
                  REFERENCE FRAME CALIBRATION
                </h2>
                <p className="text-[10px] text-[var(--bb-smoke)] mt-0.5">
                  Load a RAW (DNG) or compressed ColorChecker frame to calculate 3×3 transformation.
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

              <div className="space-y-1.5">
                <label className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">SOURCE FILE</label>
                <div className="flex gap-1.5">
                  <input
                    type="text"
                    value={referencePath}
                    onChange={(e) => setReferencePath(e.target.value)}
                    placeholder="CLICK BROWSE OR DROP FILE…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2.5 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none min-w-0"
                  />
                  <button type="button" onClick={browseReference} className="ui-btn ui-btn-secondary shrink-0">
                    <FolderOpen className="w-3 h-3" /> BROWSE
                  </button>
                </div>
                <div className="grid grid-cols-2 gap-1.5 pt-0.5">
                  <button
                    type="button"
                    onClick={inspectReference}
                    disabled={isProcessing || !referencePath || !chartRevision}
                    className="ui-btn ui-btn-secondary w-full"
                  >
                    <SearchCheck className="w-3 h-3" /> {isProcessing ? 'SCANNING…' : 'INSPECT CHART'}
                  </button>
                  <button type="button" onClick={loadSyntheticDemo} className="ui-btn ui-btn-ghost w-full">
                    <Sparkles className="w-3 h-3" /> LOAD DEMO
                  </button>
                </div>
                {inspectResult && (
                  <div className={`px-2.5 py-1.5 border text-[10px] ${inspectResult.qualityPassed ? 'border-[var(--bb-amber)] text-[var(--bb-gold)] bg-[var(--bb-panel)]' : 'border-[var(--bb-crimson)] text-[var(--bb-orange)] bg-[var(--bb-ember-dark)]/40'}`}>
                    {inspectResult.qualityPassed ? '✓ CHART PASSED QUALITY GATES' : `⚠ ${inspectResult.gateFailures.length} QUALITY GATE WARNING(S)`}
                  </div>
                )}
              </div>

              {/* Physical Chart Revision Selection */}
              <div className="space-y-1">
                <label className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">CHART REVISION (REQUIRED)</label>
                <select
                  value={chartRevision}
                  onChange={(e) => setChartRevision(e.target.value as ChartRevision | '')}
                  className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                >
                  <option value="" disabled>-- SELECT PHYSICAL CHART REVISION --</option>
                  <option value="classic-before-nov-2014">ColorChecker Classic (Pre-Nov 2014)</option>
                  <option value="classic-from-nov-2014">ColorChecker Classic / Calibrite (Post-Nov 2014)</option>
                </select>
              </div>

              {/* Quick & Dirty Mode Toggle */}
              <div className="p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-border)] space-y-1">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={quickAndDirty}
                    onChange={(e) => setQuickAndDirty(e.target.checked)}
                    className="accent-[var(--bb-amber)]"
                  />
                  <span className="text-[11px] font-bold text-[var(--bb-gold)]">
                    QUICK & DIRTY APPROXIMATION
                  </span>
                </label>
                <p className="text-[9px] text-[var(--bb-smoke)] leading-relaxed">
                  Inverts sRGB gamma non-linearities for non-RAW JPEG/PNG sources and relaxes neutral row constraints.
                </p>
              </div>

              {/* Primary Action Button */}
              <div className="pt-1">
                <button
                  type="button"
                  onClick={handleRunDerive}
                  disabled={isProcessing || !referencePath || !chartRevision}
                  className="ui-btn ui-btn-primary w-full h-9 text-xs"
                >
                  <Play className="w-3.5 h-3.5 fill-current" />
                  {isProcessing ? 'CALCULATING 3×3 FIT…' : 'DERIVE COLOR PROFILE'}
                </button>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-[11px] font-bold text-[var(--bb-gold)] tracking-wider">
                  CALIBRATION VALIDATION
                </h2>
                <p className="text-[10px] text-[var(--bb-smoke)]">
                  Verify patch error distributions and matrix condition numbers.
                </p>
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
                  className="ui-btn ui-btn-primary flex-1 h-9"
                >
                  CONTINUE TO BATCH QUEUE →
                </button>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-[11px] font-bold text-[var(--bb-gold)] tracking-wider flex items-center gap-1.5">
                  <FolderOpen className="w-3.5 h-3.5 text-[var(--bb-amber)]" />
                  BATCH PROCESSING DIRECTORY
                </h2>
                <p className="text-[10px] text-[var(--bb-smoke)]">
                  Select matching image folders for batch calibration and 16-bit TIFF export.
                </p>
              </div>

              <div className="space-y-1">
                <label className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">SOURCE FOLDER</label>
                <div className="flex gap-1.5">
                  <input
                    type="text"
                    value={batchInputPath}
                    onChange={(e) => setBatchInputPath(e.target.value)}
                    placeholder="SOURCE DIRECTORY…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2.5 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none min-w-0"
                  />
                  <button
                    type="button"
                    className="ui-btn ui-btn-secondary shrink-0"
                    onClick={async () => {
                      try {
                        const path = await chooseDirectory();
                        if (path) setBatchInputPath(path);
                      } catch {
                        setErrorMessage('Directory picker requires the desktop application.');
                      }
                    }}
                  >
                    <FolderOpen className="w-3 h-3" /> CHOOSE
                  </button>
                </div>
              </div>

              <div className="space-y-1">
                <label className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">DESTINATION FOLDER</label>
                <div className="flex gap-1.5">
                  <input
                    type="text"
                    value={batchOutputPath}
                    onChange={(e) => setBatchOutputPath(e.target.value)}
                    placeholder="OUTPUT DIRECTORY…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2.5 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none min-w-0"
                  />
                  <button
                    type="button"
                    className="ui-btn ui-btn-secondary shrink-0"
                    onClick={async () => {
                      try {
                        const path = await chooseDirectory();
                        if (path) setBatchOutputPath(path);
                      } catch {
                        setErrorMessage('Directory picker requires the desktop application.');
                      }
                    }}
                  >
                    <FolderOpen className="w-3 h-3" /> CHOOSE
                  </button>
                </div>
              </div>

              {/* Overwrite Safety Policy */}
              <div className="p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={overwriteOutputs}
                    onChange={(e) => setOverwriteOutputs(e.target.checked)}
                    className="accent-[var(--bb-amber)]"
                  />
                  <span className="text-[11px] font-bold text-[var(--bb-sand)]">
                    OVERWRITE EXISTING OUTPUTS
                  </span>
                </label>
                <p className="text-[9px] text-[var(--bb-smoke)] mt-0.5">
                  Default: Skips existing files to prevent unintended data loss.
                </p>
              </div>

              {/* Live Batch Progress Bar */}
              {isProcessing && batchProgress && (
                <div className="space-y-1.5 p-2 bg-[var(--bb-vacuum)] border border-[var(--bb-border)]">
                  <div className="flex justify-between text-[9px] text-[var(--bb-smoke)]">
                    <span className="truncate max-w-[220px]">{batchProgress.file ?? 'FINALIZING…'}</span>
                    <span>{batchProgress.completed}/{batchProgress.total}</span>
                  </div>
                  <div className="h-1 bg-[var(--bb-charcoal)] border border-[var(--bb-border)] overflow-hidden">
                    <div
                      className="h-full bg-gradient-to-r from-[var(--bb-crimson)] via-[var(--bb-orange)] to-[var(--bb-gold)] transition-all"
                      style={{ width: `${batchProgress.total > 0 ? (batchProgress.completed / batchProgress.total) * 100 : 0}%` }}
                    />
                  </div>
                </div>
              )}

              <div className="grid grid-cols-4 gap-1.5">
                <button
                  type="button"
                  onClick={handleRunBatch}
                  disabled={isProcessing || !batchInputPath || !batchOutputPath || !deriveResult}
                  className="ui-btn ui-btn-primary col-span-3 h-9"
                >
                  <Cpu className="w-3.5 h-3.5" />
                  {isProcessing ? 'PROCESSING…' : 'START BATCH'}
                </button>
                <button
                  type="button"
                  disabled={!isProcessing}
                  onClick={async () => {
                    try {
                      await backend.cancelBatch();
                    } catch (error: unknown) {
                      setErrorMessage(error instanceof Error ? error.message : String(error));
                    }
                  }}
                  className="ui-btn ui-btn-ghost h-9"
                >
                  <X className="w-3 h-3" /> STOP
                </button>
              </div>
            </div>
          )}

          {step === 4 && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2">
                <h2 className="text-[11px] font-bold text-[var(--bb-gold)] tracking-wider flex items-center gap-1.5">
                  <CheckCircle2 className="w-3.5 h-3.5 text-[var(--bb-gold)]" />
                  BATCH PROCESSING COMPLETE
                </h2>
                <p className="text-[10px] text-[var(--bb-smoke)]">
                  Summary of transformed 16-bit linear sRGB TIFF outputs.
                </p>
              </div>

              {batchSummary && (
                <div className="space-y-2.5">
                  <div className="grid grid-cols-3 gap-1.5 text-center">
                    <div className="p-1.5 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                      <div className="text-[9px] text-[var(--bb-smoke)] font-bold">SUCCEEDED</div>
                      <div className="text-sm font-bold text-[var(--bb-gold)]">
                        {batchSummary.succeeded.length}
                      </div>
                    </div>
                    <div className="p-1.5 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                      <div className="text-[9px] text-[var(--bb-smoke)] font-bold">SKIPPED</div>
                      <div className="text-sm font-bold text-[var(--bb-amber)]">
                        {batchSummary.skipped.length}
                      </div>
                    </div>
                    <div className="p-1.5 bg-[var(--bb-panel)] border border-[var(--bb-border)]">
                      <div className="text-[9px] text-[var(--bb-smoke)] font-bold">FAILED</div>
                      <div className="text-sm font-bold text-[var(--bb-crimson)]">
                        {batchSummary.failed.length}
                      </div>
                    </div>
                  </div>

                  {/* Export Options */}
                  <div className="p-2.5 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] space-y-1.5">
                    <span className="text-[9px] font-bold text-[var(--bb-smoke)] tracking-wider">EXPORT INTERCHANGE</span>
                    <div className="grid grid-cols-2 gap-1.5">
                      {(['clf', 'cube'] as const).map((format) => (
                        <button
                          key={format}
                          type="button"
                          className="ui-btn ui-btn-secondary"
                          onClick={async () => {
                            if (!deriveResult?.profilePath) {
                              setErrorMessage('Derive a profile before exporting.');
                              return;
                            }
                            try {
                              const output = await chooseSavePath(`colorbalance.${format}`, format);
                              if (output) await backend.exportProfile(deriveResult.profilePath, format, output, 33);
                            } catch (error: unknown) {
                              setErrorMessage(error instanceof Error ? error.message : String(error));
                            }
                          }}
                        >
                          <Save className="w-3 h-3" /> .{format.toUpperCase()}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Footer Diagnostic Stamp */}
          <div className="pt-2 border-t border-[var(--bb-border)] text-[8px] text-[var(--bb-ash)] flex justify-between items-center tracking-wider">
            <span>TAURI // PLANCK LIGHT-TABLE</span>
            <span>4-POINT WARP // D65</span>
          </div>
        </aside>
      </main>

      {/* Bottom Collapsible Diagnostic Console */}
      <DiagnosticConsole />
    </div>
  );
};

export default App;
