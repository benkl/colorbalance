export interface InteractionState {
  referencePath: string;
  batchInputPath: string;
  batchOutputPath: string;
  tab: 'reference' | 'validate' | 'export';
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
