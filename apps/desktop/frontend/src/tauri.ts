import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open, save } from '@tauri-apps/plugin-dialog';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { BatchSummary, ChartQuad, ChartRevision, DeriveResult, InspectResult } from './types';

export interface BatchProgress {
  completed: number;
  total: number;
  file: string | null;
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

export async function chooseImage(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Supported images', extensions: ['dng', 'jpg', 'jpeg', 'png'] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseDirectory(): Promise<string | null> {
  const selected = await open({ multiple: false, directory: true });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseSavePath(defaultPath: string, extension: string): Promise<string | null> {
  const selected = await save({
    defaultPath,
    filters: [{ name: `${extension.toUpperCase()} file`, extensions: [extension] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export function listenForBatchProgress(callback: (progress: BatchProgress) => void): Promise<UnlistenFn> {
  return listen<BatchProgress>('batch-progress', (event) => callback(event.payload));
}

export async function listenForFileDrop(
  onDrop: (paths: string[], position: { x: number; y: number }) => void,
  onHover: (hovered: boolean) => void,
): Promise<UnlistenFn> {
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === 'over') {
      onHover(true);
      return;
    }
    if (event.payload.type === 'leave') {
      onHover(false);
      return;
    }
    if (event.payload.type === 'drop') {
      onHover(false);
      onDrop(event.payload.paths, event.payload.position);
    }
  });
}
