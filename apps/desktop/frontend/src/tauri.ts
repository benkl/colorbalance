import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { logger } from './logger.ts';
import type { BatchSummary, ChartQuad, ChartRevision, DeriveResult, InspectResult } from './types';

export type { UnlistenFn };

export interface BatchProgress {
  completed: number;
  total: number;
  file: string | null;
}

export interface DroppedFiles {
  paths: string[];
  position: { x: number; y: number };
}

export interface BackendBridge {
  inspectReference(path: string, revision: ChartRevision, quad?: ChartQuad, quickAndDirty?: boolean): Promise<InspectResult>;
  deriveProfile(path: string, revision: ChartRevision, profilePath: string, reportPath?: string, quad?: ChartQuad, quickAndDirty?: boolean, force?: boolean): Promise<DeriveResult>;
  applyBatch(profilePath: string, inputPath: string, outputPath: string, overwrite?: boolean, force?: boolean): Promise<BatchSummary>;
  cancelBatch(): Promise<void>;
  exportProfile(profilePath: string, format: 'clf' | 'cube', outputPath: string, size?: number): Promise<string>;
}

export const backend: BackendBridge = {
  inspectReference: async (path, revision, quad, quickAndDirty) => {
    logger.ipc('IPC', `Invoking inspect_reference on "${path}" [${revision}] (quickAndDirty: ${Boolean(quickAndDirty)})`);
    try {
      const result = await invoke<InspectResult>('inspect_reference', {
        path,
        chartRevision: revision,
        quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
        quickAndDirty,
      });
      logger.success('IPC', `inspect_reference succeeded: ${result.imageWidth}x${result.imageHeight} (${result.qualityPassed ? 'PASS' : 'WARN'})`);
      return result;
    } catch (err: unknown) {
      logger.error('IPC', `inspect_reference failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  deriveProfile: async (path, revision, profilePath, reportPath, quad, quickAndDirty, force) => {
    logger.ipc('IPC', `Invoking derive_profile on "${path}" -> "${profilePath}"`);
    try {
      const result = await invoke<DeriveResult>('derive_profile', {
        path,
        chartRevision: revision,
        profilePath,
        reportPath,
        quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
        quickAndDirty,
        force,
      });
      logger.success('IPC', `derive_profile completed: mean ΔE = ${result.validation.meanDeltaE.toFixed(3)}, max ΔE = ${result.validation.maxDeltaE.toFixed(3)}`);
      return result;
    } catch (err: unknown) {
      logger.error('IPC', `derive_profile failed: ${err instanceof Error ? err.message : String(err)}`);
      throw err;
    }
  },
  applyBatch: async (profilePath, inputPath, outputPath, overwrite, force) => {
    logger.ipc('IPC', `Invoking apply_batch: "${inputPath}" -> "${outputPath}"`);
    try {
      const result = await invoke<BatchSummary>('apply_batch', {
        profilePath,
        inputPath,
        outputPath,
        overwrite,
        force,
      });
      logger.success('IPC', `apply_batch complete: ${result.succeeded.length} succeeded, ${result.skipped.length} skipped, ${result.failed.length} failed`);
      return result;
    } catch (err: unknown) {
      logger.error('IPC', `apply_batch failed: ${err instanceof Error ? err.message : String(err)}`);
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
