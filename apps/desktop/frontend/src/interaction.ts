import type { LibraryEntryView, LibraryGps } from './types';

export type Tab = 'reference' | 'validate' | 'export' | 'library';

export interface InteractionState {
  referencePath: string;
  batchInputPath: string;
  batchOutputPath: string;
  tab: Tab;
  errorMessage: string;
}

export function referenceFromDrop(paths: string[]): string | null {
  return paths.find((path) => /\.(dng|jpe?g|png)$/i.test(path)) ?? null;
}

export function applyDroppedReference(state: InteractionState, paths: string[]): InteractionState {
  const referencePath = referenceFromDrop(paths);
  if (!referencePath) {
    return {
      ...state,
      errorMessage: 'Drop a supported DNG, JPEG, or PNG reference image.',
    };
  }
  return {
    ...state,
    referencePath,
    tab: 'reference',
    errorMessage: '',
  };
}

export function applyDialogSelection(
  state: InteractionState,
  target: 'reference' | 'batch-input' | 'batch-output',
  path: string | null,
): InteractionState {
  if (!path) return state;
  if (target === 'reference') {
    return { ...state, referencePath: path, tab: 'reference', errorMessage: '' };
  }
  if (target === 'batch-input') {
    return { ...state, batchInputPath: path, errorMessage: '' };
  }
  return { ...state, batchOutputPath: path, errorMessage: '' };
}

export function canDerive(state: Pick<InteractionState, 'referencePath'>): boolean {
  return state.referencePath.length > 0;
}

export function canProcessBatch(
  state: Pick<InteractionState, 'batchInputPath' | 'batchOutputPath'>,
  hasProfile: boolean,
): boolean {
  return hasProfile && state.batchInputPath.length > 0 && state.batchOutputPath.length > 0;
}

/** Comma-separated tag text from the save form: trimmed, empty and case-insensitive duplicate tags dropped. */
export function parseTags(text: string): string[] {
  const seen = new Set<string>();
  const tags: string[] = [];
  for (const part of text.split(',')) {
    const tag = part.trim();
    const key = tag.toLowerCase();
    if (!tag || seen.has(key)) continue;
    seen.add(key);
    tags.push(tag);
  }
  return tags;
}

/** Latitude and longitude to four decimals, altitude in whole meters when known. */
export function formatGps(gps: LibraryGps): string {
  const base = `${gps.latitude.toFixed(4)}, ${gps.longitude.toFixed(4)}`;
  return gps.altitude === null ? base : `${base} · ${Math.round(gps.altitude)} m`;
}

export interface QualityBadge {
  label: string;
  tone: 'ok' | 'warn' | 'bad';
}

/** Quality badges for a library entry: one status badge, plus quick-and-dirty when the source was not a RAW. */
export function qualityBadges(entry: Pick<LibraryEntryView, 'qualityPassed' | 'qualityOverridden' | 'quickAndDirty'>): QualityBadge[] {
  const badges: QualityBadge[] = [];
  if (entry.qualityOverridden) badges.push({ label: 'OVERRIDDEN', tone: 'warn' });
  else if (entry.qualityPassed) badges.push({ label: 'PASSED', tone: 'ok' });
  else badges.push({ label: 'FAILED', tone: 'bad' });
  if (entry.quickAndDirty) badges.push({ label: 'QUICK & DIRTY', tone: 'warn' });
  return badges;
}

export interface LibraryCardModel {
  id: string;
  title: string;
  camera: string;
  lens: string | null;
  capturedAt: string | null;
  gps: string | null;
  badges: QualityBadge[];
  deltaE: string;
  chartRevision: string;
  decoder: string;
  tags: string[];
}

/** Display strings for one gallery card. */
export function libraryCardModel(entry: LibraryEntryView): LibraryCardModel {
  return {
    id: entry.id,
    title: entry.label,
    camera: `${entry.cameraMake} ${entry.cameraModel}`.trim(),
    lens: entry.lens,
    capturedAt: entry.capturedAt,
    gps: entry.gps ? formatGps(entry.gps) : null,
    badges: qualityBadges(entry),
    deltaE: `ΔE mean ${entry.meanDeltaE.toFixed(2)} / max ${entry.maxDeltaE.toFixed(2)}`,
    chartRevision: entry.chartRevision,
    decoder: `${entry.decoder} ${entry.decoderVersion}`.trim(),
    tags: entry.tags,
  };
}

const sameText = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** True when the entry's camera make or model differs from the known loaded camera. False when no camera is known. */
export function libraryCameraMismatch(
  entry: Pick<LibraryEntryView, 'cameraMake' | 'cameraModel'>,
  known: { make: string; model: string } | null,
): boolean {
  if (!known) return false;
  return !sameText(entry.cameraMake, known.make) || !sameText(entry.cameraModel, known.model);
}

export interface BatchProfileChoice {
  profilePath: string;
  /** Library calibrations proceed on camera or decode-contract mismatch with a warning; derived profiles stay fail-closed. */
  allowMismatch: boolean;
  source: 'library' | 'derived';
}

/** The profile the batch applies: an active library entry wins over the profile derived in this session. */
export function chooseBatchProfile(
  derivedProfilePath: string | null,
  activeLibrary: Pick<LibraryEntryView, 'profilePath'> | null,
): BatchProfileChoice | null {
  if (activeLibrary) return { profilePath: activeLibrary.profilePath, allowMismatch: true, source: 'library' };
  if (derivedProfilePath) return { profilePath: derivedProfilePath, allowMismatch: false, source: 'derived' };
  return null;
}

const LIBRARY_PATH_KEY = 'colorbalance.libraryPath';

/** The remembered library folder. Empty when none is stored or storage is unavailable (tests, locked-down webview). */
export function readStoredLibraryPath(): string {
  try {
    return typeof localStorage === 'undefined' ? '' : (localStorage.getItem(LIBRARY_PATH_KEY) ?? '');
  } catch {
    return '';
  }
}

export function storeLibraryPath(path: string): void {
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(LIBRARY_PATH_KEY, path);
  } catch {
    // Remembering the folder is a convenience; the in-memory value still works.
  }
}
