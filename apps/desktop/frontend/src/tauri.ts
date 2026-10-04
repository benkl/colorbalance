import type { BatchSummary, ChartQuad, ChartRevision, DeriveResult, InspectResult } from './types';

export type UnlistenFn = () => void;

interface TauriEvent<T> {
  payload: T;
}

interface DragDropPayload {
  type: 'over' | 'drop' | 'leave' | 'enter';
  paths?: string[];
  position?: { x: number; y: number };
}

interface TauriRuntime {
  core: {
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  };
  event: {
    listen<T>(event: string, handler: (event: TauriEvent<T>) => void): Promise<UnlistenFn>;
  };
  webview: {
    getCurrentWebview(): {
      onDragDropEvent(handler: (event: TauriEvent<DragDropPayload>) => void): Promise<UnlistenFn>;
    };
  };
  dialog: {
    open(options?: Record<string, unknown>): Promise<string | string[] | null>;
    save(options?: Record<string, unknown>): Promise<string | null>;
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

export const backend: BackendBridge = {
  inspectReference: (path, revision, quad, quickAndDirty) => runtime().core.invoke('inspect_reference', {
    path,
    chartRevision: revision,
    quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
    quickAndDirty,
  }),
  deriveProfile: (path, revision, profilePath, reportPath, quad, quickAndDirty, force) => runtime().core.invoke('derive_profile', {
    path,
    chartRevision: revision,
    profilePath,
    reportPath,
    quad: quad ? { corners: quad.map(({ x, y }) => [x, y]) } : undefined,
    quickAndDirty,
    force,
  }),
  applyBatch: (profilePath, inputPath, outputPath, overwrite, force) => runtime().core.invoke('apply_batch', {
    profilePath,
    inputPath,
    outputPath,
    overwrite,
    force,
  }),
  cancelBatch: () => runtime().core.invoke('cancel_batch'),
  exportProfile: (profilePath, format, outputPath, size) => runtime().core.invoke('export_profile', {
    profilePath,
    format,
    outputPath,
    size,
  }),
};

export async function chooseImage(): Promise<string | null> {
  const selected = await runtime().dialog.open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Supported images', extensions: ['dng', 'jpg', 'jpeg', 'png'] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseDirectory(): Promise<string | null> {
  const selected = await runtime().dialog.open({ multiple: false, directory: true });
  return typeof selected === 'string' ? selected : null;
}

export async function chooseSavePath(defaultPath: string, extension: string): Promise<string | null> {
  const selected = await runtime().dialog.save({
    defaultPath,
    filters: [{ name: `${extension.toUpperCase()} file`, extensions: [extension] }],
  });
  return typeof selected === 'string' ? selected : null;
}

export function listenForBatchProgress(callback: (progress: BatchProgress) => void): Promise<UnlistenFn> {
  return runtime().event.listen<BatchProgress>('batch-progress', (event) => callback(event.payload));
}

export function listenForFileDrop(
  onDrop: (paths: string[], position: { x: number; y: number }) => void,
  onHover: (hovered: boolean) => void,
): Promise<UnlistenFn> {
  return runtime().webview.getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === 'over' || payload.type === 'enter') {
      onHover(true);
    } else if (payload.type === 'leave') {
      onHover(false);
    } else if (payload.type === 'drop') {
      onHover(false);
      onDrop(payload.paths ?? [], payload.position ?? { x: 0, y: 0 });
    }
  });
}
