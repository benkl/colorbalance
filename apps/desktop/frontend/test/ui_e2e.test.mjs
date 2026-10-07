import assert from 'node:assert/strict';
import test from 'node:test';

test('UI end-to-end: native file drop loads image, enables derive, and completes batch workflow', async () => {
  const listeners = new Map();
  const registeredHandlers = new Map();

  // Install Tauri runtime IPC and event mock before importing App/tauri
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (cmd, args) => {
        if (registeredHandlers.has(cmd)) {
          return registeredHandlers.get(cmd)(args);
        }
        throw new Error(`Unhandled mock command: ${cmd}`);
      },
      transformCallback: (fn) => fn,
      unregisterCallback: () => undefined,
      convertFileSrc: (path) => `asset://${path}`,
    },
  };

  registeredHandlers.set('choose_image', async () => 'C:/images/20261003_183314.jpg');
  registeredHandlers.set('choose_directory', async () => 'C:/images/shoot_raws');
  registeredHandlers.set('choose_save_path', async (args) => `C:/exports/${args.defaultPath}`);
  registeredHandlers.set('plugin:event|listen', async (args) => {
    const event = args.event;
    const handler = args.handler;
    if (!listeners.has(event)) {
      listeners.set(event, new Set());
    }
    listeners.get(event).add(handler);
    return () => {
      listeners.get(event)?.delete(handler);
    };
  });
  registeredHandlers.set('plugin:event|unlisten', async () => undefined);

  registeredHandlers.set('inspect_reference', async (args) => {
    return {
      camera: {
        make: 'Rendered Image (Quick & Dirty)',
        model: 'JPG',
        decoder: 'colorbalance-rendered-jpeg',
        decoderVersion: '0.1.0',
      },
      imageWidth: 1864,
      imageHeight: 1398,
      chartRevision: args.chartRevision,
      qualityPassed: true,
      gateFailures: [],
      quad: args.quad?.corners ?? [[40, 40], [440, 34], [440, 280], [40, 280]],
    };
  });

  registeredHandlers.set('derive_profile', async (args) => {
    return {
      profilePath: args.profilePath,
      reportPath: args.reportPath,
      digest: '0396277212d10d4818b4f46510d9b587636ecedd6646af1de5770fe702cc019e',
      qualityPassed: true,
      warnings: [],
      validation: {
        meanDeltaE: 0.84,
        medianDeltaE: 0.69,
        p95DeltaE: 1.83,
        maxDeltaE: 2.14,
        neutralMaxDeltaE: 0.42,
        skinMaxDeltaE: 0.76,
        conditionNumber: 1.14,
        patchCount: 24,
      },
      patches: [],
    };
  });

  registeredHandlers.set('apply_batch', async () => {
    return {
      total: 3,
      succeeded: ['001.jpg', '002.jpg', '003.jpg'],
      skipped: [],
      failed: [],
      warnings: [{ file: '002.jpg', warning: 'decoder version drift: profile built with 1, current 2' }],
    };
  });

  const tauri = await import('../src/tauri.ts');
  const interaction = await import('../src/interaction.ts');

  // 1. Test native file choose button
  const pickedPath = await tauri.chooseImage();
  assert.equal(pickedPath, 'C:/images/20261003_183314.jpg');

  let uiState = {
    referencePath: pickedPath,
    batchInputPath: '',
    batchOutputPath: '',
    tab: 'reference',
    errorMessage: '',
  };
  assert.equal(interaction.canDerive(uiState), true);

  // 2. Test native drag-drop event delivery
  const dropPayload = {
    paths: ['C:/images/dropped_20261003_183314.jpg'],
    position: { x: 200, y: 150 },
  };
  uiState = interaction.applyDroppedReference(uiState, dropPayload.paths);
  assert.equal(uiState.referencePath, 'C:/images/dropped_20261003_183314.jpg');
  assert.equal(uiState.errorMessage, '');

  // 3. Inspect reference through backend command
  const inspected = await tauri.backend.inspectReference(
    uiState.referencePath,
    'classic-from-nov-2014',
    undefined,
  );
  assert.equal(inspected.imageWidth, 1864);
  assert.equal(inspected.imageHeight, 1398);
  assert.equal(inspected.qualityPassed, true);

  // 4. Derive profile through backend command
  const derived = await tauri.backend.deriveProfile(
    uiState.referencePath,
    'classic-from-nov-2014',
    'colorbalance.cbprofile.json',
    'report.html',
    inspected.quad.map(([x, y]) => ({ x, y })),
  );
  assert.equal(derived.validation.patchCount, 24);
  assert.equal(derived.digest.length, 64);

  // 5. Select batch input and output directories via native pickers
  const batchIn = await tauri.chooseDirectory();
  const batchOut = await tauri.chooseDirectory();
  uiState = interaction.applyDialogSelection(uiState, 'batch-input', batchIn);
  uiState = interaction.applyDialogSelection(uiState, 'batch-output', batchOut);
  assert.equal(interaction.canProcessBatch(uiState, true), true);

  // 6. Execute batch apply
  const batchSummary = await tauri.backend.applyBatch(
    derived.profilePath,
    uiState.batchInputPath,
    uiState.batchOutputPath,
    false,
    'display-p3',
  );
  assert.equal(batchSummary.total, 3);
  assert.equal(batchSummary.succeeded.length, 3);
  assert.deepEqual(batchSummary.warnings, [
    { file: '002.jpg', warning: 'decoder version drift: profile built with 1, current 2' },
  ]);
});

test('chart detection bridge keeps status and corners separate from revision and preview', async () => {
  const calls = [];
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (command, args) => {
        calls.push([command, args]);
        return { status: 'ambiguous' };
      },
      transformCallback: (fn) => fn,
      unregisterCallback: () => undefined,
    },
  };
  const { backend } = await import('../src/tauri.ts');
  assert.deepEqual(await backend.detectChart('ref.dng'), { status: 'ambiguous' });
  assert.deepEqual(calls, [['detect_chart', { path: 'ref.dng' }]]);
});

test('correct image: IPC carries the output path and returns backend-rendered previews', async () => {
  const calls = [];
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (cmd, args) => {
        calls.push([cmd, args]);
        if (cmd === 'release_previews') return undefined;
        assert.equal(cmd, 'correct_image');
        return {
          beforePath: '/session/before.png',
          afterPath: '/session/after.png',
          outputPath: args.outputPath ?? null,
          warnings: [],
        };
      },
      transformCallback: (fn) => fn,
      unregisterCallback: () => undefined,
      convertFileSrc: (path) => `asset://${path}`,
    },
  };
  const tauri = await import('../src/tauri.ts');
  const saved = await tauri.backend.correctImage('p.cbprofile.json', 'in.dng', 'out.tiff', false, 'adobe-rgb');
  assert.deepEqual(calls[0], ['correct_image', {
    profilePath: 'p.cbprofile.json',
    inputPath: 'in.dng',
    outputPath: 'out.tiff',
    overwrite: false,
    outputSpace: 'adobe-rgb',
  }]);
  assert.equal(saved.outputPath, 'out.tiff');
  const preview = await tauri.backend.correctImage('p.cbprofile.json', 'in.dng');
  assert.equal(preview.outputPath, null);
  assert.equal(preview.beforeUrl, 'asset:///session/before.png');
  assert.equal(preview.afterUrl, 'asset:///session/after.png');
  await tauri.releasePreviewUrls([preview.afterUrl, 'asset:///outside.png']);
  assert.deepEqual(calls[2], ['release_previews', { paths: ['/session/after.png'] }]);
});
