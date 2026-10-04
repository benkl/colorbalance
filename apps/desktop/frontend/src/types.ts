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
  decoderVersion: string;
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
  reportPath: string;
  digest: string;
  validation: ValidationSummary;
  patches: PatchValidation[];
  qualityPassed: boolean;
  warnings: string[];
}

export interface BatchSummary {
  succeeded: string[];
  skipped: string[];
  failed: Array<{ file: string; error: string }>;
  total: number;
}
