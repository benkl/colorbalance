import React, { useEffect, useRef, useState } from 'react';
import { DEFAULT_EXPORT_OPTIONS, JPEG_SAMPLINGS, OUTPUT_SPACES } from './types';
import type { ChartQuad, ChartRevision, CorrectResult, DeriveResult, BatchSummary, ExportFormat, ExportOptions, InspectResult, JpegSampling, LibraryEntryView, LibraryListingView, MetadataSummary, OutputSpace } from './types';
import { backend, releasePreviewUrls, chooseDirectory, chooseImage, chooseSavePath, listenForBatchProgress, listenForFileDrop, listenForFileDropHover, listenForOperationProgress } from './tauri';
import type { BatchProgress, OperationProgress } from './tauri';
import { chooseBatchProfile, parseTags, readStoredLibraryPath, referenceFromDrop, storeLibraryPath } from './interaction';
import type { Tab } from './interaction';
import { CameraMismatchNotice, LibraryGallery, LibraryPanel } from './components/Library';
import { LightTableOverlay } from './components/LightTableOverlay';
import { ValidationPanel } from './components/ValidationPanel';
import { QualityFailures } from './components/QualityFailures';
import { BeforeAfter } from './components/BeforeAfter';
import { DiagnosticConsole } from './components/DiagnosticConsole';
import { logger } from './logger';
import {
  Layers,
  Sparkles,
  FolderOpen,
  Play,
  Cpu,
  AlertCircle,
  Upload,
  SearchCheck,
  X,
  Save,
  Library as LibraryIcon,
} from 'lucide-react';

const decodePreview = async (url: string): Promise<void> => {
  const image = new Image();
  image.src = url;
  await image.decode();
};

const MetadataLines: React.FC<{ metadata: MetadataSummary }> = ({ metadata }) => (
  <div className="text-[9px] space-y-0.5" data-testid="metadata-summary">
    <div className="text-[var(--bb-sand)] break-words">Metadata copied: {metadata.copied.length > 0 ? metadata.copied.join(', ') : 'none'}</div>
    {metadata.skipped.length > 0 && (
      <div className="text-[var(--bb-orange)] break-words">Metadata skipped: {metadata.skipped.join(', ')}</div>
    )}
  </div>
);


const TABS: { id: Tab; label: string; icon: typeof Layers }[] = [
  { id: 'reference', label: 'REFERENCE', icon: Layers },
  { id: 'validate', label: 'VALIDATE', icon: Sparkles },
  { id: 'export', label: 'EXPORT', icon: Save },
  { id: 'library', label: 'LIBRARY', icon: LibraryIcon },
];

export const App: React.FC = () => {
  const [tab, setTab] = useState<Tab>('reference');

  // Workflow State
  const [referencePath, setReferencePath] = useState<string>('');
  const [referencePreview, setReferencePreview] = useState<string>('');
  // True pixel size of the displayed image; chart coordinates live in this space.
  const [imageSize, setImageSize] = useState<{ width: number; height: number }>({ width: 480, height: 320 });
  const [chartRevision, setChartRevision] = useState<ChartRevision | ''>('');
  const [compare, setCompare] = useState<(CorrectResult & { source: string }) | null>(null);
  const retainedPreviews = useRef(new Set<string>());
  const visiblePreviews = useRef(new Set<string>());
  useEffect(() => {
    const active = new Set([referencePreview, ...(compare ? [compare.beforeUrl, compare.afterUrl] : [])].filter((url): url is string => Boolean(url)));
    visiblePreviews.current = active;
    active.forEach((url) => retainedPreviews.current.add(url));
    let cancelled = false;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (cancelled) return;
      const retired = [...retainedPreviews.current].filter((url) => !visiblePreviews.current.has(url));
      if (retired.length) {
        void releasePreviewUrls(retired).then(() => {
          retired.forEach((url) => {
            if (!visiblePreviews.current.has(url)) retainedPreviews.current.delete(url);
          });
        }).catch((error: unknown) => logger.warn('IPC', `Preview cleanup failed: ${String(error)}`));
      }
    }));
    return () => { cancelled = true; };
  });

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
  const [detectionStatus, setDetectionStatus] = useState<'idle' | 'running' | 'found' | 'missing' | 'ambiguous' | 'error'>('idle');
  const [detectionError, setDetectionError] = useState('');
  const referenceGeneration = useRef(0);
  const selectionGeneration = useRef(0);
  const detectionGeneration = useRef(0);
  const loadedReferencePath = useRef<string | null>(null);
  const [loadedPath, setLoadedPath] = useState<string | null>(null);
  const referenceInputRef = useRef<HTMLInputElement>(null);

  // Batch Processing State
  const [batchInputPath, setBatchInputPath] = useState<string>('');
  const [batchOutputPath, setBatchOutputPath] = useState<string>('');
  const [overwriteOutputs, setOverwriteOutputs] = useState<boolean>(false);
  const [exportSettings, setExportSettings] = useState<Omit<ExportOptions, 'overwrite'>>({
    space: DEFAULT_EXPORT_OPTIONS.space,
    format: DEFAULT_EXPORT_OPTIONS.format,
    quality: DEFAULT_EXPORT_OPTIONS.quality,
    sampling: DEFAULT_EXPORT_OPTIONS.sampling,
    includeXmpIptc: DEFAULT_EXPORT_OPTIONS.includeXmpIptc,
    stripGps: DEFAULT_EXPORT_OPTIONS.stripGps,
  });
  const updateExport = (patch: Partial<Omit<ExportOptions, 'overwrite'>>) =>
    setExportSettings((current) => ({ ...current, ...patch }));
  const [batchSummary, setBatchSummary] = useState<BatchSummary | null>(null);
  const [batchProgress, setBatchProgress] = useState<BatchProgress | null>(null);
  const [operationProgress, setOperationProgress] = useState<OperationProgress | null>(null);

  // Library State
  const [libraryPath, setLibraryPath] = useState<string>(readStoredLibraryPath);
  const [libraryListing, setLibraryListing] = useState<LibraryListingView | null>(null);
  const [libraryLoading, setLibraryLoading] = useState(false);
  const [libraryError, setLibraryError] = useState('');
  const [librarySelectedId, setLibrarySelectedId] = useState<string | null>(null);
  const [activeLibrary, setActiveLibrary] = useState<LibraryEntryView | null>(null);
  const libraryScan = useRef(0);
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveLabel, setSaveLabel] = useState('');
  const [saveNotes, setSaveNotes] = useState('');
  const [saveTags, setSaveTags] = useState('');
  const [saveGps, setSaveGps] = useState(true);
  const [saveStatus, setSaveStatus] = useState<{ kind: 'ok' | 'error'; text: string } | null>(null);
  const [saving, setSaving] = useState(false);

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
      showBrowserFile(file);
      setTab('reference');
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
        void showReference(supported);
        setTab('reference');
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

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listenForOperationProgress(setOperationProgress)
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => {
        // Browser preview mode
      });
    return () => unlisten?.();
  }, []);

  /** The backend runs commands off the UI thread; these bracket one so the progress strip and disabled buttons track it. */
  const beginWork = () => {
    setIsProcessing(true);
    setOperationProgress(null);
    setBatchProgress(null);
  };
  const endWork = () => {
    setIsProcessing(false);
    setOperationProgress(null);
  };

  /** Default chart rectangle: 8% margin on every side of the image. */
  const defaultQuad = (width: number, height: number): ChartQuad => {
    const mx = Math.round(width * 0.08);
    const my = Math.round(height * 0.08);
    return [
      { x: mx, y: my },
      { x: width - mx, y: my },
      { x: width - mx, y: height - my },
      { x: mx, y: height - my },
    ];
  };

  const detectReference = async (path: string, referenceId: number) => {
    const detectionId = ++detectionGeneration.current;
    setDetectionStatus('running');
    setDetectionError('');
    try {
      const result = await backend.detectChart(path);
      if (referenceId !== referenceGeneration.current || detectionId !== detectionGeneration.current || loadedReferencePath.current !== path) return;
      setDetectionStatus(result.status);
      if (result.status === 'found') {
        setQuad(result.quad.map(([x, y]) => ({ x, y })) as ChartQuad);
        setInspectResult(null);
      }
    } catch (error: unknown) {
      if (referenceId !== referenceGeneration.current || detectionId !== detectionGeneration.current) return;
      setDetectionStatus('error');
      setDetectionError(error instanceof Error ? error.message : String(error));
    }
  };

  const adjustQuad = (next: ChartQuad) => {
    ++detectionGeneration.current;
    setQuad(next);
    setDetectionStatus('idle');
    setInspectResult(null);
  };

  const startReference = () => {
    ++selectionGeneration.current;
    ++detectionGeneration.current;
    loadedReferencePath.current = null;
    setLoadedPath(null);
    setCompare(null);
    setDetectionStatus('idle');
    setDetectionError('');
    setDeriveResult(null);
    setActiveLibrary(null);
    return ++referenceGeneration.current;
  };

  /** Native path: Rust decodes (EXIF-upright) and returns a PNG preview plus true dimensions. */
  const showReference = async (path: string) => {
    const referenceId = startReference();
    setReferencePath(path);
    setErrorMessage('');
    setInspectResult(null);
    beginWork();
    try {
      const loaded = await backend.loadReference(path);
      if (referenceId !== referenceGeneration.current) {
        if (!visiblePreviews.current.has(loaded.previewUrl)) void releasePreviewUrls([loaded.previewUrl]);
        return;
      }
      try {
        await decodePreview(loaded.previewUrl);
      } catch (error) {
        if (!visiblePreviews.current.has(loaded.previewUrl)) void releasePreviewUrls([loaded.previewUrl]);
        throw error;
      }
      if (referenceId !== referenceGeneration.current) {
        if (!visiblePreviews.current.has(loaded.previewUrl)) void releasePreviewUrls([loaded.previewUrl]);
        return;
      }
      retainedPreviews.current.add(loaded.previewUrl);
      loadedReferencePath.current = path;
      setLoadedPath(path);
      setReferencePreview(loaded.previewUrl);
      setImageSize({ width: loaded.imageWidth, height: loaded.imageHeight });
      setQuad(loaded.quad.map(([x, y]) => ({ x, y })) as ChartQuad);
      logger.success('UI', `Preview ready: ${loaded.imageWidth}x${loaded.imageHeight}`);
      if (loaded.imageWidth * loaded.imageHeight <= 12_000_000) {
        // Let the preview paint before starting thumbnail analysis.
        const cornersId = detectionGeneration.current;
        requestAnimationFrame(() => setTimeout(() => {
          if (referenceId === referenceGeneration.current && cornersId === detectionGeneration.current) {
            void detectReference(path, referenceId);
          }
        }, 0));
      }
    } catch (err: unknown) {
      if (referenceId !== referenceGeneration.current) return;
      setErrorMessage(`Could not load image: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      if (referenceId === referenceGeneration.current) endWork();
    }
  };

  /** Browser path: the webview decodes the file, so its natural size is authoritative. */
  const showBrowserFile = (file: File) => {
    const referenceId = startReference();
    endWork();
    const url = URL.createObjectURL(file);
    const probe = new Image();
    probe.onload = () => {
      if (referenceId !== referenceGeneration.current) {
        URL.revokeObjectURL(url);
        return;
      }
      setReferencePath(file.name);
      setReferencePreview(url);
      setImageSize({ width: probe.naturalWidth, height: probe.naturalHeight });
      setQuad(defaultQuad(probe.naturalWidth, probe.naturalHeight));
      setInspectResult(null);
      setErrorMessage('');
    };
    probe.onerror = () => {
      if (referenceId !== referenceGeneration.current) return;
      URL.revokeObjectURL(url);
      setErrorMessage(`The browser cannot display "${file.name}". Use the desktop app for DNG files.`);
    };
    probe.src = url;
  };
  const browseReference = async () => {
    const selectionId = ++selectionGeneration.current;
    logger.info('UI', 'Action: Browse reference frame');
    try {
      const selected = await chooseImage();
      if (selectionId !== selectionGeneration.current) return;
      if (selected) {
        logger.success('UI', `Reference selected via native picker: "${selected}"`);
        await showReference(selected);
      }
    } catch (err: unknown) {
      if (selectionId !== selectionGeneration.current) return;
      logger.warn('UI', `Native dialog unavailable (${err instanceof Error ? err.message : String(err)}); falling back to HTML file input`);
      referenceInputRef.current?.click();
    }
  };

  const handleBrowserReference = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    logger.info('UI', `HTML file input selected: ${file.name}`);
    showBrowserFile(file);
    event.target.value = '';
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
    const referenceId = referenceGeneration.current;
    ++detectionGeneration.current;
    setDetectionStatus('idle');
    beginWork();
    setErrorMessage('');
    try {
      const result = await backend.inspectReference(referencePath, chartRevision, quad);
      if (referenceId !== referenceGeneration.current) {
        if (result.previewUrl && !visiblePreviews.current.has(result.previewUrl)) void releasePreviewUrls([result.previewUrl]);
        return;
      }
      if (result.previewUrl) {
        try {
          await decodePreview(result.previewUrl);
        } catch (error) {
          if (!visiblePreviews.current.has(result.previewUrl)) void releasePreviewUrls([result.previewUrl]);
          throw error;
        }
        if (referenceId !== referenceGeneration.current) {
          if (!visiblePreviews.current.has(result.previewUrl)) void releasePreviewUrls([result.previewUrl]);
          return;
        }
        retainedPreviews.current.add(result.previewUrl);
        setReferencePreview(result.previewUrl);
      }
      setInspectResult(result);
      setQuad(result.quad.map(([x, y]) => ({ x, y })) as ChartQuad);
    } catch (error: unknown) {
      if (referenceId === referenceGeneration.current) setErrorMessage(error instanceof Error ? error.message : String(error));
    } finally {
      if (referenceId === referenceGeneration.current) endWork();
    }
  };

  const loadSyntheticDemo = () => {
    const referenceId = startReference();
    endWork();
    // Measure the displayed size (EXIF-upright) rather than assuming the stored one.
    const probe = new Image();
    probe.onload = () => {
      if (referenceId !== referenceGeneration.current) return;
      setReferencePath('20261003_183314.jpg');
      setReferencePreview(probe.src);
      setImageSize({ width: probe.naturalWidth, height: probe.naturalHeight });
      setQuad(defaultQuad(probe.naturalWidth, probe.naturalHeight));
    };
    probe.src = '/test-data/20261003_183314.jpg';
    setChartRevision('classic-from-nov-2014');
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
      qualityOverride: false,
      gateFailures: [],
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
    beginWork();
    void (async () => {
      try {
        const result = await backend.deriveProfile(
          referencePath,
          chartRevision,
          'colorbalance_profile.cbprofile.json',
          'colorbalance_report.html',
          quad,
        );
        setDeriveResult(result);
        setCompare(null);
        previewAttempt.current = '';
        setTab('validate');
      } catch (error: unknown) {
        if (typeof window !== 'undefined' && !('__TAURI__' in window)) {
          logger.warn('UI', 'Browser preview: loaded synthetic demo calibration');
          loadSyntheticDemo();
          setTab('validate');
        } else {
          setErrorMessage(error instanceof Error ? error.message : String(error));
        }
      } finally {
        endWork();
      }
    })();
  };

  /** The active Library entry wins over the session profile; single images still fail closed. */
  const exportProfile = chooseBatchProfile(deriveResult?.profilePath ?? null, activeLibrary);
  const runCorrect = async (input: string, save: boolean) => {
    if (!exportProfile) {
      setErrorMessage('Derive a profile or choose a library entry first.');
      return;
    }
    setErrorMessage('');
    beginWork();
    try {
      let output: string | undefined;
      if (save) {
        const name = (input.split(/[\\/]/).pop() ?? 'image').replace(/\.[^.]+$/, '');
        const extension = exportSettings.format === 'jpeg' ? 'jpg' : 'tiff';
        const chosen = await chooseSavePath(`${name}_corrected.${extension}`, extension);
        if (!chosen) return;
        output = chosen;
      }
      const result = await backend.correctImage(exportProfile.profilePath, input, output, { ...exportSettings, overwrite: true }, false);
      try {
        await Promise.all([decodePreview(result.beforeUrl), decodePreview(result.afterUrl)]);
      } catch (error) {
        const unmounted = [result.beforeUrl, result.afterUrl].filter((url) => !visiblePreviews.current.has(url));
        void releasePreviewUrls(unmounted);
        throw error;
      }
      retainedPreviews.current.add(result.beforeUrl);
      retainedPreviews.current.add(result.afterUrl);
      setCompare({ ...result, source: input });
    } catch (error: unknown) {
      setErrorMessage(error instanceof Error ? error.message : String(error));
    } finally {
      endWork();
    }
  };

  const handleCorrectSingle = async () => {
    try {
      const picked = await chooseImage();
      if (picked) await runCorrect(picked, true);
    } catch {
      setErrorMessage('Image picker requires the desktop application.');
    }
  };

  // Preview the derived reference only. A Library profile may belong to a different camera.
  const previewAttempt = useRef('');
  useEffect(() => {
    if (tab === 'reference' || tab === 'library' || activeLibrary || compare || isProcessing || !deriveResult?.profilePath || !referencePath) return;
    const key = `${deriveResult.profilePath}|${deriveResult.digest}|${referencePath}`;
    if (previewAttempt.current === key) return;
    previewAttempt.current = key;
    void runCorrect(referencePath, false);
  });

  const loadedCamera = inspectResult ? { make: inspectResult.camera.make, model: inspectResult.camera.model } : null;

  const handleRunBatch = () => {
    if (!batchInputPath || !batchOutputPath || !exportProfile) {
      setErrorMessage('Select source and destination folders, and derive a profile or choose a library entry.');
      return;
    }
    setErrorMessage('');
    beginWork();
    void (async () => {
      try {
        const result = await backend.applyBatch(
          exportProfile.profilePath,
          batchInputPath,
          batchOutputPath,
          { ...exportSettings, overwrite: overwriteOutputs },
          exportProfile.allowMismatch,
        );
        setBatchSummary(result);
      } catch (error: unknown) {
        setErrorMessage(error instanceof Error ? error.message : String(error));
      } finally {
        endWork();
      }
    })();
  };

  const refreshLibrary = async (path: string) => {
    const scanId = ++libraryScan.current;
    if (!path) {
      setLibraryListing(null);
      return;
    }
    setLibraryLoading(true);
    setLibraryError('');
    try {
      const listing = await backend.listLibrary(path);
      if (scanId !== libraryScan.current) return;
      setLibraryListing(listing);
      setLibrarySelectedId((current) => (listing.entries.some((entry) => entry.id === current) ? current : null));
    } catch (error: unknown) {
      if (scanId !== libraryScan.current) return;
      setLibraryListing(null);
      setLibraryError(error instanceof Error ? error.message : String(error));
    } finally {
      if (scanId === libraryScan.current) setLibraryLoading(false);
    }
  };

  // The folder is the source of truth: rescan every time the tab is opened.
  const openTab = (next: Tab) => {
    setTab(next);
    if (next === 'library') void refreshLibrary(libraryPath);
  };

  const changeLibraryPath = (path: string) => {
    ++libraryScan.current;
    setLibraryPath(path);
    setLibraryListing(null);
    setLibrarySelectedId(null);
    setActiveLibrary(null);
    setLibraryError('');
    setLibraryLoading(false);
    storeLibraryPath(path);
  };

  const chooseLibraryFolder = async () => {
    try {
      const path = await chooseDirectory();
      if (!path) return;
      changeLibraryPath(path);
      setActiveLibrary(null);
      await refreshLibrary(path);
    } catch {
      setErrorMessage('Directory picker requires the desktop application.');
    }
  };

  const useLibraryEntry = (entry: LibraryEntryView) => {
    setActiveLibrary(entry);
    setBatchSummary(null);
    setCompare(null);
  };

  const openSaveForm = () => {
    setSaveStatus(null);
    setSaveOpen(true);
  };

  const handleSaveToLibrary = async () => {
    if (!deriveResult || !referencePath) return;
    if (!saveLabel.trim()) {
      setSaveStatus({ kind: 'error', text: 'Enter a label for the library entry.' });
      return;
    }
    let folder = libraryPath;
    setSaving(true);
    setSaveStatus(null);
    try {
      if (!folder) {
        const picked = await chooseDirectory();
        if (!picked) {
          setSaveStatus({ kind: 'error', text: 'Choose a library folder to save into.' });
          return;
        }
        folder = picked;
        changeLibraryPath(picked);
      }
      const entry = await backend.saveToLibrary({
        libraryPath: folder,
        profilePath: deriveResult.profilePath,
        referencePath,
        label: saveLabel.trim(),
        notes: saveNotes,
        tags: parseTags(saveTags),
        includeGps: saveGps,
      });
      setSaveStatus({ kind: 'ok', text: `Saved "${entry.label}" to the library (${entry.id}).` });
      setSaveOpen(false);
      setSaveLabel('');
      setSaveNotes('');
      setSaveTags('');
    } catch (error: unknown) {
      setSaveStatus({ kind: 'error', text: error instanceof Error ? error.message : String(error) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="w-screen h-screen flex flex-col bg-[var(--bb-space)] text-[var(--bb-sand)] font-mono select-none overflow-hidden">
      <header className="h-10 shrink-0 border-b border-[var(--bb-border)] bg-[var(--bb-vacuum)] px-3 flex items-center justify-between text-xs">
        <div className="flex items-center gap-2">
          <span className="font-bold text-[var(--bb-sand)] text-xs">ColorBalance</span>
          <span className="text-[9px] text-[var(--bb-smoke)]">v0.1.0</span>
        </div>

        {/* Workspace tabs */}
        <nav className="flex items-center gap-1">
          {TABS.map((item) => {
            const Icon = item.icon;
            const isActive = tab === item.id;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => openTab(item.id)}
                className={`h-7 px-2.5 flex items-center gap-1.5 text-[10px] font-bold border-b cursor-pointer transition-colors ${
                  isActive
                    ? 'text-[var(--bb-gold)] border-[var(--bb-gold)]'
                    : 'text-[var(--bb-smoke)] border-transparent hover:text-[var(--bb-sand)]'
                }`}
              >
                <Icon className="w-3 h-3" />
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="text-[10px] text-[var(--bb-smoke)]">ENGINE READY</div>
      </header>

      {/* Live progress: stage of the running single-image command, or finished/total for a batch. */}
      {isProcessing && (
        <div data-testid="progress-strip" className="shrink-0 px-3 py-1.5 bg-[var(--bb-vacuum)] border-b border-[var(--bb-border)] flex items-center gap-3 text-[10px] text-[var(--bb-smoke)]">
          <span data-testid="progress-label" className="font-bold text-[var(--bb-amber)] tracking-wider shrink-0">
            {batchProgress ? 'BATCH' : operationProgress ? operationProgress.stage.toUpperCase() : 'WORKING'}…
          </span>
          <div className="flex-1 h-1 bg-[var(--bb-charcoal)] border border-[var(--bb-border)] overflow-hidden">
            <div
              data-testid="progress-bar"
              className={`h-full bg-gradient-to-r from-[var(--bb-crimson)] via-[var(--bb-orange)] to-[var(--bb-gold)] transition-all ${batchProgress || operationProgress ? '' : 'animate-pulse w-full'}`}
              style={
                batchProgress
                  ? { width: `${batchProgress.total > 0 ? (batchProgress.completed / batchProgress.total) * 100 : 0}%` }
                  : operationProgress
                    ? { width: `${((operationProgress.step - 1) / operationProgress.steps) * 100}%` }
                    : undefined
              }
            />
          </div>
          <span data-testid="progress-count" className="shrink-0 tabular-nums">
            {batchProgress
              ? `${batchProgress.completed}/${batchProgress.total} FILES`
              : operationProgress
                ? `STEP ${operationProgress.step}/${operationProgress.steps}`
                : ''}
          </span>
          {batchProgress?.file && (
            <span className="truncate max-w-[260px]">{batchProgress.file.split(/[\\/]/).pop()}</span>
          )}
        </div>
      )}

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
          {tab === 'library' ? (
            <LibraryGallery
              listing={libraryListing}
              libraryPath={libraryPath}
              loading={libraryLoading}
              selectedId={librarySelectedId}
              activeId={activeLibrary?.id ?? null}
              onSelect={setLibrarySelectedId}
            />
          ) : tab !== 'reference' && compare ? (
            <BeforeAfter
              key={compare.source}
              beforeSrc={compare.beforeUrl}
              afterSrc={compare.afterUrl}
              onClose={() => { setCompare(null); setTab('reference'); }}
            />
          ) : (
            <LightTableOverlay
              imageSrc={referencePreview}
              imageWidth={imageSize.width}
              imageHeight={imageSize.height}
              quad={quad}
              onQuadChange={adjustQuad}
              onQuadInteractionStart={() => { ++detectionGeneration.current; setDetectionStatus('idle'); }}
              onBrowse={browseReference}
              disabled={isProcessing}
            />
          )}
        </div>

        {/* Workspace controls */}
        <aside className="w-[380px] shrink-0 h-full bg-[var(--bb-surface)] p-4 flex flex-col overflow-y-auto border-l border-[var(--bb-border)]">
          {tab === 'reference' && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2 space-y-1">
                <h2 className="text-xs font-bold text-[var(--bb-sand)]">Reference</h2>
                <p className="text-[10px] text-[var(--bb-smoke)]">Load a ColorChecker frame, check its corners, then derive a profile.</p>
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
                    onChange={(e) => { startReference(); setReferencePath(e.target.value); setReferencePreview(''); setInspectResult(null); }}
                    placeholder="CLICK BROWSE OR DROP FILE…"
                    className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2.5 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none min-w-0"
                  />
                  <button type="button" onClick={browseReference} className="ui-btn ui-btn-secondary shrink-0">
                    <FolderOpen className="w-3 h-3" /> BROWSE
                  </button>
                </div>
                <button
                  type="button"
                  onClick={() => { if (loadedPath) void detectReference(loadedPath, referenceGeneration.current); }}
                  disabled={!loadedPath || detectionStatus === 'running' || isProcessing}
                  className="ui-btn ui-btn-ghost w-full"
                >
                  <SearchCheck className="w-3 h-3" /> {detectionStatus === 'running' ? 'FINDING CHART…' : 'FIND CHART / RETRY'}
                </button>
                <p role="status" className="text-[10px] text-[var(--bb-smoke)]">
                  {detectionStatus === 'found' && 'Chart corners proposed on the light table. Check and drag them if needed; select the physical chart revision yourself.'}
                  {detectionStatus === 'missing' && 'No chart found. Set the corners manually or retry.'}
                  {detectionStatus === 'ambiguous' && 'Multiple possible charts found. Set the corners manually; no chart was chosen.'}
                  {detectionStatus === 'error' && `Detection failed: ${detectionError}. Set the corners manually or retry.`}
                  {detectionStatus === 'running' && 'Looking for a chart; the corners remain editable.'}
                  {detectionStatus === 'idle' && loadedPath && imageSize.width * imageSize.height > 12_000_000 && 'Large image: automatic detection skipped. Use Find Chart to start it.'}
                  {detectionStatus === 'idle' && referencePreview && !loadedPath && 'Detection needs a desktop-loaded file. Set corners manually in browser preview mode.'}
                </p>
                <div className="grid grid-cols-2 gap-1.5 pt-0.5">
                  <button
                    type="button"
                    onClick={inspectReference}
                    disabled={isProcessing || detectionStatus === 'running' || !referencePath || !chartRevision}
                    className="ui-btn ui-btn-secondary w-full"
                  >
                    <SearchCheck className="w-3 h-3" /> {isProcessing ? 'SCANNING…' : 'INSPECT CHART'}
                  </button>
                  <button type="button" onClick={loadSyntheticDemo} className="ui-btn ui-btn-ghost w-full">
                    <Sparkles className="w-3 h-3" /> LOAD DEMO
                  </button>
                </div>
                {inspectResult && (
                  <div className={`border-l-2 pl-2 text-[10px] space-y-1.5 ${inspectResult.qualityPassed ? 'border-[var(--bb-amber)] text-[var(--bb-gold)]' : 'border-[var(--bb-crimson)] text-[var(--bb-orange)]'}`}>
                    <div>
                      {inspectResult.qualityPassed
                        ? '✓ CHART PASSED QUALITY GATES'
                        : `⚠ ${inspectResult.gateFailures.length} QUALITY WARNING(S): deriving still works`}
                    </div>
                    {inspectResult.gateFailures.length > 0 && (
                      <details>
                        <summary className="cursor-pointer text-[9px] text-[var(--bb-smoke)]">DETAILS</summary>
                        <div className="pt-1.5">
                          <QualityFailures failures={inspectResult.gateFailures} />
                        </div>
                      </details>
                    )}
                  </div>
                )}
              </div>

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

              <div className="pt-1">
                <button
                  type="button"
                  onClick={handleRunDerive}
                  disabled={isProcessing || detectionStatus === 'running' || !referencePath || !chartRevision}
                  className="ui-btn ui-btn-primary w-full h-9 text-xs"
                >
                  <Play className="w-3.5 h-3.5 fill-current" />
                  {isProcessing ? 'CALCULATING 3×3 FIT…' : 'DERIVE COLOR PROFILE'}
                </button>
              </div>
            </div>
          )}

          {tab === 'library' && (
            <LibraryPanel
              listing={libraryListing}
              libraryPath={libraryPath}
              loading={libraryLoading}
              error={libraryError}
              selected={libraryListing?.entries.find((entry) => entry.id === librarySelectedId) ?? null}
              active={activeLibrary}
              loadedCamera={loadedCamera}
              onLibraryPathChange={changeLibraryPath}
              onChoose={() => void chooseLibraryFolder()}
              onRefresh={() => void refreshLibrary(libraryPath)}
              onUse={useLibraryEntry}
            />
          )}

          {tab === 'validate' && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2 space-y-1">
                <h2 className="text-xs font-bold text-[var(--bb-sand)]">Validate</h2>
                <p className="text-[10px] text-[var(--bb-smoke)]">Check patch errors and capture warnings before using the profile.</p>
              </div>

              <ValidationPanel
                validation={deriveResult?.validation}
                patches={deriveResult?.patches}
                warnings={deriveResult?.warnings}
                qualityPassed={deriveResult?.qualityPassed}
                gateFailures={deriveResult?.gateFailures}
              />

              <div className="space-y-1.5 border-t border-[var(--bb-border)] pt-3" data-testid="save-library-panel">
                <button
                  type="button"
                  onClick={openSaveForm}
                  disabled={!deriveResult || !referencePath || saving}
                  className="ui-btn ui-btn-secondary w-full"
                  data-testid="save-to-library"
                >
                  <LibraryIcon className="w-3 h-3" /> SAVE TO LIBRARY…
                </button>
                {saveOpen && (
                  <div className="space-y-1.5" data-testid="save-library-form">
                    <label htmlFor="library-label" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">LABEL</label>
                    <input
                      id="library-label"
                      type="text"
                      value={saveLabel}
                      onChange={(e) => setSaveLabel(e.target.value)}
                      className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                    />
                    <label htmlFor="library-notes" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">NOTES (OPTIONAL)</label>
                    <textarea
                      id="library-notes"
                      value={saveNotes}
                      onChange={(e) => setSaveNotes(e.target.value)}
                      rows={2}
                      className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                    />
                    <label htmlFor="library-tags" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">TAGS (COMMA-SEPARATED)</label>
                    <input
                      id="library-tags"
                      type="text"
                      value={saveTags}
                      onChange={(e) => setSaveTags(e.target.value)}
                      className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                    />
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={saveGps}
                        onChange={(e) => setSaveGps(e.target.checked)}
                        className="accent-[var(--bb-amber)]"
                        data-testid="save-include-gps"
                      />
                      <span className="text-[10px] text-[var(--bb-sand)]">Include GPS location from the reference</span>
                    </label>
                    <p className="text-[9px] text-[var(--bb-smoke)]">
                      Saves to {libraryPath || 'a library folder you will be asked to choose'}.
                    </p>
                    <div className="flex gap-1.5">
                      <button
                        type="button"
                        onClick={() => void handleSaveToLibrary()}
                        disabled={saving}
                        className="ui-btn ui-btn-primary flex-1"
                        data-testid="save-library-confirm"
                      >
                        {saving ? 'SAVING…' : 'SAVE'}
                      </button>
                      <button type="button" onClick={() => setSaveOpen(false)} className="ui-btn ui-btn-ghost">
                        CANCEL
                      </button>
                    </div>
                  </div>
                )}
                {saveStatus && (
                  <div
                    className={`text-[10px] break-words ${saveStatus.kind === 'ok' ? 'text-[var(--bb-gold)]' : 'text-[var(--bb-orange)]'}`}
                    data-testid="save-library-status"
                  >
                    {saveStatus.kind === 'ok' ? '✓ ' : '⚠ '}{saveStatus.text}
                  </div>
                )}
              </div>
            </div>
          )}

          {tab === 'export' && (
            <div className="space-y-3.5">
              <div className="border-b border-[var(--bb-border)] pb-2 space-y-1">
                <h2 className="text-xs font-bold text-[var(--bb-sand)]">Export</h2>
                <p className="text-[10px] text-[var(--bb-smoke)]">Apply the active profile to an image or folder. Inputs are never changed.</p>
              </div>

              {exportProfile ? (
                <div className="border-b border-[var(--bb-border)] pb-2 space-y-1 text-[10px]" data-testid="profile-files">
                  <div className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">ACTIVE PROFILE · {exportProfile.source === 'library' ? 'LIBRARY' : 'DERIVED'}</div>
                  <div className="text-[var(--bb-gold)] break-all">{activeLibrary?.label ?? exportProfile.profilePath}</div>
                  {activeLibrary && <div className="text-[var(--bb-smoke)] break-all">{exportProfile.profilePath}</div>}
                  {deriveResult && exportProfile.source === 'derived' && <div className="text-[var(--bb-smoke)] break-all">Report: {deriveResult.reportPath}</div>}
                  {activeLibrary && (
                    <>
                      <button type="button" className="text-[var(--bb-amber)] underline" onClick={() => { setActiveLibrary(null); setCompare(null); }}>Clear Library selection</button>
                      <p className="text-[var(--bb-smoke)]">Single images block camera or decode mismatches. Batches continue with per-file warnings; exposure mismatches still block.</p>
                      <CameraMismatchNotice entry={activeLibrary} loadedCamera={loadedCamera} />
                    </>
                  )}
                </div>
              ) : (
                <p className="text-[10px] text-[var(--bb-smoke)]">Derive a profile in Reference or select one in Library.</p>
              )}

              <div className="space-y-1.5 border-b border-[var(--bb-border)] pb-3" data-testid="output-space-panel">
                <h3 className="text-[10px] font-bold text-[var(--bb-sand)]">Output settings</h3>
                <label htmlFor="export-format" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">FILE FORMAT</label>
                <select
                  id="export-format"
                  value={exportSettings.format}
                  onChange={(e) => updateExport({ format: e.target.value as ExportFormat })}
                  className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                >
                  <option value="tiff">TIFF, 16-bit</option>
                  <option value="jpeg">JPEG, 8-bit</option>
                </select>
                <label htmlFor="output-space" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">COLOR SPACE</label>
                <select
                  id="output-space"
                  value={exportSettings.space}
                  onChange={(e) => updateExport({ space: e.target.value as OutputSpace })}
                  className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                >
                  {OUTPUT_SPACES.map((space) => (
                    <option key={space.id} value={space.id}>{space.label}</option>
                  ))}
                </select>
                {exportSettings.format === 'jpeg' && (
                  <div className="space-y-1.5" data-testid="jpeg-options">
                    <label htmlFor="jpeg-quality" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">JPEG QUALITY (1-100)</label>
                    <input
                      id="jpeg-quality"
                      type="number"
                      min={1}
                      max={100}
                      step={1}
                      value={exportSettings.quality}
                      onChange={(e) => {
                        const value = Math.round(Number(e.target.value));
                        if (Number.isFinite(value)) updateExport({ quality: Math.min(100, Math.max(1, value)) });
                      }}
                      className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                    />
                    <label htmlFor="jpeg-sampling" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">CHROMA SUBSAMPLING</label>
                    <select
                      id="jpeg-sampling"
                      value={exportSettings.sampling}
                      onChange={(e) => updateExport({ sampling: e.target.value as JpegSampling })}
                      className="w-full bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none"
                    >
                      {JPEG_SAMPLINGS.map((sampling) => (
                        <option key={sampling.id} value={sampling.id}>{sampling.label}</option>
                      ))}
                    </select>
                  </div>
                )}
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={exportSettings.includeXmpIptc}
                    onChange={(e) => updateExport({ includeXmpIptc: e.target.checked })}
                    className="accent-[var(--bb-amber)]"
                  />
                  <span className="text-[10px] text-[var(--bb-sand)]">Copy XMP and IPTC</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={exportSettings.stripGps}
                    onChange={(e) => updateExport({ stripGps: e.target.checked })}
                    className="accent-[var(--bb-amber)]"
                  />
                  <span className="text-[10px] text-[var(--bb-sand)]">Strip GPS location</span>
                </label>
                <p className="text-[9px] text-[var(--bb-smoke)]">Saved files embed a matching ICC profile and copy basic camera EXIF. On-screen previews use sRGB.</p>
              </div>

              <div className="space-y-2 border-b border-[var(--bb-border)] pb-3" data-testid="correct-panel">
                <div>
                  <h3 className="text-[10px] font-bold text-[var(--bb-sand)]">One image</h3>
                  <p className="text-[9px] text-[var(--bb-smoke)]">Choose an image, then choose where to write the corrected file.</p>
                </div>
                <button type="button" onClick={handleCorrectSingle} disabled={isProcessing || !exportProfile} className="ui-btn ui-btn-primary w-full" data-testid="correct-single">
                  <FolderOpen className="w-3 h-3" /> APPLY &amp; SAVE IMAGE…
                </button>
                {deriveResult && !activeLibrary && referencePath && (
                  <button type="button" onClick={() => void runCorrect(referencePath, true)} disabled={isProcessing} className="ui-btn ui-btn-ghost w-full" data-testid="save-reference">
                    <Save className="w-3 h-3" /> SAVE REFERENCE…
                  </button>
                )}
                {compare && (
                  <div className="text-[10px] space-y-0.5" data-testid="correct-result">
                    {compare.outputPath && <div className="text-[var(--bb-gold)] break-all">✓ SAVED {compare.outputPath}</div>}
                    {compare.metadata && <MetadataLines metadata={compare.metadata} />}
                    {compare.warnings.map((w, i) => (
                      <div key={i} className="text-[var(--bb-orange)]">⚠ {w}</div>
                    ))}
                  </div>
                )}
              </div>

              <details className="border-b border-[var(--bb-border)] pb-3 text-[10px]">
                <summary className="cursor-pointer text-[var(--bb-smoke)]">Interchange files (.CLF / .cube)</summary>
                <div className="grid grid-cols-2 gap-1.5 pt-2">
                  {(['clf', 'cube'] as const).map((format) => (
                    <button
                      key={format}
                      type="button"
                      disabled={!exportProfile}
                      className="ui-btn ui-btn-secondary"
                      onClick={async () => {
                        if (!exportProfile) return;
                        try {
                          const output = await chooseSavePath(`colorbalance.${format}`, format);
                          if (output) await backend.exportProfile(exportProfile.profilePath, format, output, 33);
                        } catch (error: unknown) {
                          setErrorMessage(error instanceof Error ? error.message : String(error));
                        }
                      }}
                    >
                      <Save className="w-3 h-3" /> .{format.toUpperCase()}
                    </button>
                  ))}
                </div>
              </details>

              <div className="space-y-2.5">
                <div>
                  <h3 className="text-[10px] font-bold text-[var(--bb-sand)]">Folder batch</h3>
                  <p className="text-[9px] text-[var(--bb-smoke)]">Apply to supported images in a source folder; write to a separate folder.</p>
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

                <div className="grid grid-cols-4 gap-1.5">
                  <button
                    type="button"
                    onClick={handleRunBatch}
                    disabled={isProcessing || !batchInputPath || !batchOutputPath || !exportProfile}
                    className="ui-btn ui-btn-primary col-span-3 h-9"
                  >
                    <Cpu className="w-3.5 h-3.5" />
                    {isProcessing ? 'PROCESSING…' : 'APPLY & WRITE BATCH'}
                  </button>
                  <button
                    type="button"
                    disabled={!isProcessing || !batchProgress}
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

                    {batchSummary.failed.length > 0 && (
                      <div
                        className="p-2.5 bg-[var(--bb-ember-dark)]/40 border border-[var(--bb-crimson)] space-y-1"
                        data-testid="batch-failures"
                      >
                        <div className="text-[10px] font-bold text-[var(--bb-orange)] tracking-wider">
                          FAILED FILES
                        </div>
                        <ul className="space-y-1 text-[9px] text-[var(--bb-sand)] max-h-40 overflow-y-auto">
                          {batchSummary.failed.map((item) => (
                            <li key={item.file}>
                              <span className="text-[var(--bb-gold)] break-all">{item.file}</span>
                              <div className="text-[var(--bb-orange)]">{item.error}</div>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}

                    {batchSummary.warnings.length > 0 && (
                      <div
                        className="p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-orange)] space-y-1"
                        data-testid="batch-warnings"
                      >
                        <div className="text-[10px] font-bold text-[var(--bb-orange)] tracking-wider">
                          WARNINGS ({batchSummary.warnings.length})
                        </div>
                        <ul className="space-y-1 text-[9px] text-[var(--bb-sand)] max-h-40 overflow-y-auto">
                          {batchSummary.warnings.map((item, i) => (
                            <li key={`${item.file}-${i}`}>
                              <span className="text-[var(--bb-gold)] break-all">{item.file}</span>
                              <div className="text-[var(--bb-orange)]">⚠ {item.warning}</div>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                    {batchSummary.metadata.length > 0 && (
                      <ul className="space-y-1 text-[9px] text-[var(--bb-sand)] max-h-40 overflow-y-auto" data-testid="batch-metadata">
                        {batchSummary.metadata.map((item) => (
                          <li key={item.file}>
                            <span className="text-[var(--bb-gold)] break-all">{item.file}</span>
                            <MetadataLines metadata={item} />
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </div>
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
