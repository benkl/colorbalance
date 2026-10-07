/** Encoded color space of exported images; the app previews always render sRGB. */
export type OutputSpace = "srgb" | "display-p3" | "adobe-rgb";

export type ExportFormat = "tiff" | "jpeg";
export type JpegSampling = "444" | "422" | "420";

export const JPEG_SAMPLINGS: ReadonlyArray<{ id: JpegSampling; label: string }> = [
  { id: "444", label: "4:4:4 (sharpest color)" },
  { id: "422", label: "4:2:2" },
  { id: "420", label: "4:2:0 (smallest)" },
];

/** Settings shared by single-image and batch export. */
export interface ExportOptions {
  overwrite: boolean;
  space: OutputSpace;
  format: ExportFormat;
  /** JPEG quality, 1-100. Ignored for TIFF. */
  quality: number;
  sampling: JpegSampling;
  includeXmpIptc: boolean;
  stripGps: boolean;
}

export const DEFAULT_EXPORT_OPTIONS: ExportOptions = {
  overwrite: false,
  space: "srgb",
  format: "tiff",
  quality: 95,
  sampling: "444",
  includeXmpIptc: false,
  stripGps: false,
};

/** Source metadata that reached the output, and what was left out. */
export interface MetadataSummary {
  copied: string[];
  skipped: string[];
}

export const OUTPUT_SPACES: ReadonlyArray<{ id: OutputSpace; label: string }> = [
  { id: "srgb", label: "sRGB" },
  { id: "display-p3", label: "Display P3" },
  { id: "adobe-rgb", label: "Adobe RGB (1998)" },
];

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
  previewUrl?: string;
}

/** Result of decoding a reference image for display, before any calibration. */
export interface LoadedReference {
  imageWidth: number;
  imageHeight: number;
  quad: [[number, number], [number, number], [number, number], [number, number]];
  previewUrl: string;
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
  beforeUrl: string;
  afterUrl: string;
  /** Path of the written image, or null for a preview-only run. */
  outputPath: string | null;
  /** Metadata outcome of the written image, or null for a preview-only run. */
  metadata: MetadataSummary | null;
  warnings: string[];
}

/** Non-fatal per-file notice from a batch run (e.g. decoder version drift against the profile). */
export interface BatchWarning {
  file: string;
  warning: string;
}

export interface BatchSummary {
  succeeded: string[];
  skipped: string[];
  failed: Array<{ file: string; error: string }>;
  warnings: BatchWarning[];
  /** Per-file metadata outcome for every written file. */
  metadata: Array<MetadataSummary & { file: string }>;
  total: number;
}

export interface LibraryGps {
  latitude: number;
  longitude: number;
  altitude: number | null;
}

/** One verified library entry as the backend sends it. */
export interface LibraryEntry {
  id: string;
  label: string;
  notes: string;
  tags: string[];
  profilePath: string;
  previewPath: string | null;
  digest: string;
  cameraMake: string;
  cameraModel: string;
  lens: string | null;
  capturedAt: string | null;
  gps: LibraryGps | null;
  chartRevision: string;
  decoder: string;
  decoderVersion: string;
  qualityPassed: boolean;
  qualityOverridden: boolean;
  quickAndDirty: boolean;
  meanDeltaE: number;
  maxDeltaE: number;
  patchCount: number;
}

/** A library sub-folder that could not be listed as usable; it is reported, never offered for use. */
export interface LibraryProblem {
  id: string;
  message: string;
}

export interface LibraryListing {
  entries: LibraryEntry[];
  problems: LibraryProblem[];
}

/** A library entry as the UI uses it: the preview path is turned into an asset URL. */
export type LibraryEntryView = Omit<LibraryEntry, 'previewPath'> & { previewUrl: string | null };

export interface LibraryListingView {
  entries: LibraryEntryView[];
  problems: LibraryProblem[];
}

export interface SaveToLibraryRequest {
  libraryPath: string;
  profilePath: string;
  referencePath: string;
  label: string;
  notes: string;
  tags: string[];
  includeGps: boolean;
}
