import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
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
  inspectReference: (path, revision, quad, quickAndDirty) => invoke('inspect_reference', {
    path,
    chartRevision: revision,
    quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
    quickAndDirty,
  }),
  deriveProfile: (path, revision, profilePath, reportPath, quad, quickAndDirty, force) => invoke('derive_profile', {
    path,
    chartRevision: revision,
    profilePath,
    reportPath,
    quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
    quickAndDirty,
    force,
  }),
  applyBatch: (profilePath, inputPath, outputPath, overwrite, force) => invoke('apply_batch', {
    profilePath,
    inputPath,
    outputPath,
    overwrite,
    force,
  }),
  cancelBatch: () => invoke('cancel_batch'),
  exportProfile: (profilePath, format, outputPath, size) => invoke('export_profile', {
    profilePath,
    format,
    outputPath,
    size,
  }),
};

export function chooseImage(): Promise<string | null> {
  return invoke('choose_image');
}

export function chooseDirectory(): Promise<string | null> {
  return invoke('choose_directory');
}

export function chooseSavePath(defaultPath: string, extension: string): Promise<string | null> {
  return invoke('choose_save_path', { defaultPath, extension });
}

export function listenForBatchProgress(callback: (progress: BatchProgress) => void): Promise<UnlistenFn> {
  return listen<BatchProgress>('batch-progress', (event) => callback(event.payload));
}

export function listenForFileDrop(callback: (event: DroppedFiles) => void): Promise<UnlistenFn> {
  return listen<DroppedFiles>('native-file-drop', (event) => callback(event.payload));
}

export function listenForFileDropHover(callback: (hovered: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>('native-file-drop-hover', (event) => callback(event.payload));
}
