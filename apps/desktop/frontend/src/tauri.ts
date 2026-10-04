import type { BatchSummary, ChartQuad, ChartRevision, DeriveResult, InspectResult } from './types';

export type UnlistenFn = () => void;

interface TauriRuntime {
  core: {
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  };
  event: {
    listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn>;
  };
}

declare global {
  interface Window {
    __TAURI__?: TauriRuntime;
  }
}

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

function runtime(): TauriRuntime {
  if (!window.__TAURI__) {
    throw new Error('Native desktop runtime unavailable. Start with cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml.');
  }
  return window.__TAURI__;
}

function invokeNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return runtime().core.invoke<T>(command, args);
}

export const backend: BackendBridge = {
  inspectReference: (path, revision, quad, quickAndDirty) => invokeNative('inspect_reference', {
    path,
    chartRevision: revision,
    quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
    quickAndDirty,
  }),
  deriveProfile: (path, revision, profilePath, reportPath, quad, quickAndDirty, force) => invokeNative('derive_profile', {
    path,
    chartRevision: revision,
    profilePath,
    reportPath,
    quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
    quickAndDirty,
    force,
  }),
  applyBatch: (profilePath, inputPath, outputPath, overwrite, force) => invokeNative('apply_batch', {
    profilePath,
    inputPath,
    outputPath,
    overwrite,
    force,
  }),
  cancelBatch: () => invokeNative('cancel_batch'),
  exportProfile: (profilePath, format, outputPath, size) => invokeNative('export_profile', {
    profilePath,
    format,
    outputPath,
    size,
  }),
};

export function chooseImage(): Promise<string | null> {
  return invokeNative('choose_image');
}

export function chooseDirectory(): Promise<string | null> {
  return invokeNative('choose_directory');
}

export function chooseSavePath(defaultPath: string, extension: string): Promise<string | null> {
  return invokeNative('choose_save_path', { defaultPath, extension });
}

export function listenForBatchProgress(callback: (progress: BatchProgress) => void): Promise<UnlistenFn> {
  if (!window.__TAURI__) return Promise.resolve(() => undefined);
  return runtime().event.listen<BatchProgress>('batch-progress', (event) => callback(event.payload));
}
export function listenForFileDrop(callback: (event: DroppedFiles) => void): Promise<UnlistenFn> {
  if (!window.__TAURI__) return Promise.resolve(() => undefined);
  return runtime().event.listen<DroppedFiles>('native-file-drop', (event) => callback(event.payload));
}
export function listenForFileDropHover(callback: (hovered: boolean) => void): Promise<UnlistenFn> {
  if (!window.__TAURI__) return Promise.resolve(() => undefined);
  return runtime().event.listen<boolean>('native-file-drop-hover', (event) => callback(event.payload));
}
