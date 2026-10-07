export type ChartRevision = "classic-before-nov-2014" | "classic-from-nov-2014";

export interface Point {
  x: number;
  y: number;
}

export type ChartQuad = [Point, Point, Point, Point]; // TL, TR, BR, BL

export interface CameraIdentity {
  make: string;
  model: string;
  decoder: string;
  // The core serializes CameraIdentity kebab-case, so this key is hyphenated.
  'decoder-version': string;
}

export interface InspectGateFailure {
  patch: string | null;
  reason: string;
  measured: string;
}

export interface InspectResult {
  camera: CameraIdentity;
  imageWidth: number;
  imageHeight: number;
  chartRevision: string;
  qualityPassed: boolean;
  gateFailures: InspectGateFailure[];
  quad: [[number, number], [number, number], [number, number], [number, number]];
  previewDataUrl?: string;
}

/** Result of decoding a reference image for display, before any calibration. */
export interface LoadedReference {
  imageWidth: number;
  imageHeight: number;
  quad: [[number, number], [number, number], [number, number], [number, number]];
  previewDataUrl: string;
}

/** Detection proposes corners only. It cannot identify the physical chart revision. */
export type DetectResult =
  | { status: 'found'; quad: [[number, number], [number, number], [number, number], [number, number]] }
  | { status: 'missing' | 'ambiguous' };

export interface PatchValidation {
  patch: string;
  sourceRgb: [number, number, number];
  correctedRgb: [number, number, number];
  targetRgb: [number, number, number];
  deltaE: number;
}

export interface ValidationSummary {
  meanDeltaE: number;
  medianDeltaE: number;
  p95DeltaE: number;
  maxDeltaE: number;
  neutralMaxDeltaE: number;
  skinMaxDeltaE: number;
  conditionNumber: number;
  patchCount: number;
}

export interface DeriveResult {
  profilePath: string;
  reportPath: string | null;
  digest: string;
  validation: ValidationSummary;
  patches: PatchValidation[];
  qualityPassed: boolean;
  /** True when gates failed and the profile was derived under an explicit override. */
  qualityOverride: boolean;
  gateFailures: InspectGateFailure[];
  warnings: string[];
}

/** Result of correcting one image: downscaled PNG previews rendered by the backend. */
export interface CorrectResult {
  beforeDataUrl: string;
  afterDataUrl: string;
  /** Path of the written TIFF, or null for a preview-only run. */
  outputPath: string | null;
  warnings: string[];
}

export interface BatchSummary {
  succeeded: string[];
  skipped: string[];
  failed: Array<{ file: string; error: string }>;
  total: number;
}
