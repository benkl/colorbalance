import assert from 'node:assert/strict';
import test from 'node:test';

const interaction = await import('../dist-test/interaction.js');

const initial = {
  referencePath: '',
  batchInputPath: '',
  batchOutputPath: '',
  tab: 'validate',
  errorMessage: '',
};

test('native drag-drop selects the first supported image and returns to the reference tab', () => {
  const next = interaction.applyDroppedReference(initial, [
    'C:/incoming/readme.txt',
    'C:/incoming/reference.dng',
    'C:/incoming/second.jpg',
  ]);
  assert.equal(next.referencePath, 'C:/incoming/reference.dng');
  assert.equal(next.tab, 'reference');
  assert.equal(next.errorMessage, '');
  assert.equal(interaction.canDerive(next), true);
});

test('unsupported drag reports a recoverable message without replacing reference path', () => {
  const next = interaction.applyDroppedReference(initial, ['C:/incoming/readme.txt']);
  assert.equal(next.referencePath, '');
  assert.match(next.errorMessage, /supported DNG, JPEG, or PNG/);
});

test('native dialogs populate reference, source, and output fields', () => {
  let state = interaction.applyDialogSelection(initial, 'reference', 'C:/shoot/chart.jpg');
  state = interaction.applyDialogSelection(state, 'batch-input', 'C:/shoot/raws');
  state = interaction.applyDialogSelection(state, 'batch-output', 'C:/shoot/balanced');
  assert.equal(state.referencePath, 'C:/shoot/chart.jpg');
  assert.equal(state.batchInputPath, 'C:/shoot/raws');
  assert.equal(state.batchOutputPath, 'C:/shoot/balanced');
  assert.equal(interaction.canProcessBatch(state, true), true);
  assert.equal(interaction.canProcessBatch(state, false), false);
});
