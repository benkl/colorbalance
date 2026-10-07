import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { logger } from './logger.ts';
import type { BatchSummary, ChartQuad, ChartRevision, CorrectResult, DeriveResult, DetectResult, ExportOptions, InspectResult, LibraryEntry, LibraryEntryView, LibraryListing, LibraryListingView, LoadedReference, SaveToLibraryRequest } from './types';
type NativeLoadedReference = Omit<LoadedReference, 'previewUrl'> & { previewPath: string };
type NativeInspectResult = Omit<InspectResult, 'previewUrl'> & { previewPath?: string };
type NativeCorrectResult = Omit<CorrectResult, 'beforeUrl' | 'afterUrl'> & { beforePath: string; afterPath: string };
const assetPaths = new Map<string, string>();

function assetUrl(path: string): string {
  const url = convertFileSrc(path);
  assetPaths.set(url, path);
  return url;
}

function toLibraryEntryView({ previewPath, ...entry }: LibraryEntry): LibraryEntryView {
  return { ...entry, previewUrl: previewPath ? assetUrl(previewPath) : null };
}

export async function releasePreviewUrls(urls: string[]): Promise<void> {
  const paths = urls.map((url) => assetPaths.get(url)).filter((path): path is string => Boolean(path));
  if (!paths.length) return;
  await invoke('release_previews', { paths });
  urls.forEach((url) => assetPaths.delete(url));
}

export type { UnlistenFn };

export interface BatchProgress {
  completed: number;
  total: number;
  file: string | null;
}

/** Stage report from a single-image command (`load`, `inspect`, `derive`, `correct`). */
export interface OperationProgress {
  operation: 'load' | 'inspect' | 'derive' | 'correct';
  stage: string;
  step: number;
  steps: number;
}

export interface DroppedFiles {
  paths: string[];
  position: { x: number; y: number };
}

export interface BackendBridge {
  loadReference(path: string): Promise<LoadedReference>;
  detectChart(path: string): Promise<DetectResult>;
  inspectReference(path: string, revision: ChartRevision, quad?: ChartQuad): Promise<InspectResult>;
  deriveProfile(path: string, revision: ChartRevision, profilePath: string, reportPath?: string, quad?: ChartQuad): Promise<DeriveResult>;
  correctImage(profilePath: string, inputPath: string, outputPath: string | undefined, options: ExportOptions, allowMismatch: boolean): Promise<CorrectResult>;
  applyBatch(profilePath: string, inputPath: string, outputPath: string, options: ExportOptions, allowMismatch: boolean): Promise<BatchSummary>;
  listLibrary(libraryPath: string): Promise<LibraryListingView>;
  saveToLibrary(request: SaveToLibraryRequest): Promise<LibraryEntryView>;
  cancelBatch(): Promise<void>;
  exportProfile(profilePath: string, format: 'clf' | 'cube', outputPath: string, size?: number): Promise<string>;
}

export const backend: BackendBridge = {
  loadReference: async (path) => {
    logger.ipc('IPC', `Invoking load_reference on "${path}"`);
    try {
      const { previewPath, ...result } = await invoke<NativeLoadedReference>('load_reference', { path });
      logger.success('IPC', `load_reference succeeded: ${result.imageWidth}x${result.imageHeight}`);
      return { ...result, previewUrl: assetUrl(previewPath) };
    } catch (err: unknown) {
      logger.error('IPC', `load_reference failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  detectChart: (path) => invoke<DetectResult>('detect_chart', { path }),
  inspectReference: async (path, revision, quad) => {
    logger.ipc('IPC', `Invoking inspect_reference on "${path}" [${revision}]`);
    try {
      const { previewPath, ...result } = await invoke<NativeInspectResult>('inspect_reference', {
        path,
        chartRevision: revision,
        quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
      });
      logger.success('IPC', `inspect_reference succeeded: ${result.imageWidth}x${result.imageHeight} (${result.qualityPassed ? 'PASS' : 'WARN'})`);
      return { ...result, previewUrl: previewPath ? assetUrl(previewPath) : undefined };
    } catch (err: unknown) {
      logger.error('IPC', `inspect_reference failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  deriveProfile: async (path, revision, profilePath, reportPath, quad) => {
    logger.ipc('IPC', `Invoking derive_profile on "${path}" -> "${profilePath}"`);
    try {
      const result = await invoke<DeriveResult>('derive_profile', {
        path,
        chartRevision: revision,
        profilePath,
        reportPath,
        quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
      });
      logger.success('IPC', `derive_profile completed: mean ΔE = ${result.validation.meanDeltaE.toFixed(3)}, max ΔE = ${result.validation.maxDeltaE.toFixed(3)}`);
      return result;
    } catch (err: unknown) {
      logger.error('IPC', `derive_profile failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  correctImage: async (profilePath, inputPath, outputPath, options, allowMismatch) => {
    logger.ipc('IPC', `Invoking correct_image: "${inputPath}"${outputPath ? ` -> "${outputPath}"` : ' (preview only)'}`);
    try {
      const { beforePath, afterPath, ...result } = await invoke<NativeCorrectResult>('correct_image', {
        profilePath,
        inputPath,
        outputPath,
        exportOptions: options,
        allowMismatch,
      });
      logger.success('IPC', `correct_image complete${result.outputPath ? `: wrote "${result.outputPath}"` : ''}`);
      return { ...result, beforeUrl: assetUrl(beforePath), afterUrl: assetUrl(afterPath) };
    } catch (err: unknown) {
      logger.error('IPC', `correct_image failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  applyBatch: async (profilePath, inputPath, outputPath, options, allowMismatch) => {
    logger.ipc('IPC', `Invoking apply_batch: "${inputPath}" -> "${outputPath}"`);
    try {
      const result = await invoke<BatchSummary>('apply_batch', {
        profilePath,
        inputPath,
        outputPath,
        exportOptions: options,
        allowMismatch,
      });
      logger.success('IPC', `apply_batch complete: ${result.succeeded.length} succeeded, ${result.skipped.length} skipped, ${result.failed.length} failed`);
      for (const failure of result.failed) {
        logger.error('IPC', `apply_batch failed for "${failure.file}": ${failure.error}`);
      }
      return result;
    } catch (err: unknown) {
      logger.error('IPC', `apply_batch failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  listLibrary: async (libraryPath) => {
    logger.ipc('IPC', `Invoking list_library on "${libraryPath}"`);
    try {
      const listing = await invoke<LibraryListing>('list_library', { libraryPath });
      logger.success('IPC', `list_library: ${listing.entries.length} entries, ${listing.problems.length} problems`);
      return { entries: listing.entries.map(toLibraryEntryView), problems: listing.problems };
    } catch (err: unknown) {
      logger.error('IPC', `list_library failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  saveToLibrary: async (request) => {
    logger.ipc('IPC', `Invoking save_to_library: "${request.label}" -> "${request.libraryPath}"`);
    try {
      const entry = await invoke<LibraryEntry>('save_to_library', { ...request });
      logger.success('IPC', `save_to_library saved entry "${entry.id}"`);
      return toLibraryEntryView(entry);
    } catch (err: unknown) {
      logger.error('IPC', `save_to_library failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  cancelBatch: async () => {
    logger.warn('IPC', 'Invoking cancel_batch');
    return invoke('cancel_batch');
  },
  exportProfile: async (profilePath, format, outputPath, size) => {
    logger.ipc('IPC', `Invoking export_profile: .${format} -> "${outputPath}"`);
    try {
      const result = await invoke<string>('export_profile', {
        profilePath,
        format,
        outputPath,
        size,
      });
      logger.success('IPC', `Exported .${format} to "${outputPath}"`);
      return result;
    } catch (err: unknown) {
      logger.error('IPC', `export_profile failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
};

export async function chooseImage(): Promise<string | null> {
  logger.info('DIALOG', 'Opening native image selector dialog');
  try {
    const result = await invoke<string | null>('choose_image');
    if (result) {
      logger.success('DIALOG', `Selected reference image: "${result}"`);
    } else {
      logger.info('DIALOG', 'Image selection cancelled by user');
    }
    return result;
  } catch (err: unknown) {
    logger.error('DIALOG', `choose_image failed: ${err instanceof Error ? err.message : String(err)}`);
    throw err;
  }
}

export async function chooseDirectory(): Promise<string | null> {
  logger.info('DIALOG', 'Opening native folder selector dialog');
  try {
    const result = await invoke<string | null>('choose_directory');
    if (result) {
      logger.success('DIALOG', `Selected folder: "${result}"`);
    } else {
      logger.info('DIALOG', 'Folder selection cancelled by user');
    }
    return result;
  } catch (err: unknown) {
    logger.error('DIALOG', `choose_directory failed: ${err instanceof Error ? err.message : String(err)}`);
    throw err;
  }
}

export async function chooseSavePath(defaultPath: string, extension: string): Promise<string | null> {
  logger.info('DIALOG', `Opening native save dialog for .${extension}`);
  try {
    const result = await invoke<string | null>('choose_save_path', { defaultPath, extension });
    if (result) {
      logger.success('DIALOG', `Save target chosen: "${result}"`);
    }
    return result;
  } catch (err: unknown) {
    logger.error('DIALOG', `choose_save_path failed: ${err instanceof Error ? err.message : String(err)}`);
    throw err;
  }
}

export function listenForBatchProgress(callback: (progress: BatchProgress) => void): Promise<UnlistenFn> {
  return listen<BatchProgress>('batch-progress', (event) => {
    logger.info('PROGRESS', `Batch item ${event.payload.completed}/${event.payload.total}: ${event.payload.file ?? 'DONE'}`);
    callback(event.payload);
  });
}

export function listenForOperationProgress(callback: (progress: OperationProgress) => void): Promise<UnlistenFn> {
  return listen<OperationProgress>('operation-progress', (event) => {
    logger.info('PROGRESS', `${event.payload.operation}: ${event.payload.stage} (${event.payload.step}/${event.payload.steps})`);
    callback(event.payload);
  });
}

export function listenForFileDrop(callback: (event: DroppedFiles) => void): Promise<UnlistenFn> {
  return listen<DroppedFiles>('native-file-drop', (event) => {
    logger.success('DRAG-DROP', `Received native window drop: ${event.payload.paths.join(', ')}`);
    callback(event.payload);
  });
}

export function listenForFileDropHover(callback: (hovered: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>('native-file-drop-hover', (event) => {
    if (event.payload) {
      logger.info('DRAG-DROP', 'File hovering over light-table window');
    }
    callback(event.payload);
  });
}
