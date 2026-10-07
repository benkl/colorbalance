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

  const applyBatchCalls = [];
  registeredHandlers.set('apply_batch', async (args) => {
    applyBatchCalls.push(args);
    return {
      total: 3,
      succeeded: ['001.jpg', '002.jpg', '003.jpg'],
      skipped: [],
      failed: [],
      warnings: [{ file: '002.jpg', warning: 'decoder version drift: profile built with 1, current 2' }],
      metadata: ['001.jpg', '002.jpg', '003.jpg'].map((file) => ({ file, copied: ['Exif Make'], skipped: [] })),
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
  const batchOptions = {
    overwrite: false,
    space: 'display-p3',
    format: 'jpeg',
    quality: 95,
    sampling: '444',
    includeXmpIptc: false,
    stripGps: true,
  };
  const batchSummary = await tauri.backend.applyBatch(
    derived.profilePath,
    uiState.batchInputPath,
    uiState.batchOutputPath,
    batchOptions,
    false,
  );
  assert.equal(batchSummary.total, 3);
  assert.equal(batchSummary.metadata.length, 3);
  assert.deepEqual(batchSummary.metadata[0], { file: '001.jpg', copied: ['Exif Make'], skipped: [] });
  assert.deepEqual(applyBatchCalls, [{
    profilePath: derived.profilePath,
    inputPath: uiState.batchInputPath,
    outputPath: uiState.batchOutputPath,
    exportOptions: batchOptions,
    allowMismatch: false,
  }]);
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
  const jpegOptions = {
    overwrite: false,
    space: 'adobe-rgb',
    format: 'jpeg',
    quality: 82,
    sampling: '420',
    includeXmpIptc: true,
    stripGps: true,
  };
  const saved = await tauri.backend.correctImage('p.cbprofile.json', 'in.dng', 'out.jpg', jpegOptions, false);
  assert.deepEqual(calls[0], ['correct_image', {
    profilePath: 'p.cbprofile.json',
    inputPath: 'in.dng',
    outputPath: 'out.jpg',
    exportOptions: jpegOptions,
    allowMismatch: false,
  }]);
  assert.equal(saved.outputPath, 'out.jpg');
  const preview = await tauri.backend.correctImage('p.cbprofile.json', 'in.dng', undefined, { ...jpegOptions, format: 'tiff' }, false);
  assert.equal(preview.outputPath, null);
  assert.equal(preview.beforeUrl, 'asset:///session/before.png');
  assert.equal(preview.afterUrl, 'asset:///session/after.png');
  await tauri.releasePreviewUrls([preview.afterUrl, 'asset:///outside.png']);
  assert.deepEqual(calls[2], ['release_previews', { paths: ['/session/after.png'] }]);
});

const libraryEntry = {
  id: 'sony-a7-studio-0396277a',
  label: 'Studio A7',
  notes: 'Strobe, 5600K\nsecond line',
  tags: ['studio', 'strobe'],
  profilePath: 'C:/library/sony-a7-studio-0396277a/profile.cbprofile.json',
  previewPath: 'C:/library/sony-a7-studio-0396277a/preview.png',
  digest: '0396277212d10d4818b4f46510d9b587636ecedd6646af1de5770fe702cc019e',
  cameraMake: 'SONY',
  cameraModel: 'ILCE-7M3',
  lens: 'FE 35mm F1.8',
  capturedAt: '2026-10-03T18:33:14',
  gps: { latitude: 52.520008, longitude: 13.404954, altitude: 34.2 },
  chartRevision: 'classic-from-november2014',
  decoder: 'rawler-ahd',
  decoderVersion: '0.7.1',
  qualityPassed: true,
  qualityOverridden: false,
  quickAndDirty: false,
  meanDeltaE: 0.84,
  maxDeltaE: 2.14,
  patchCount: 24,
};

test('library: listing becomes cards, using an entry applies its profile with allowMismatch and surfaces the warning', async () => {
  const calls = [];
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (cmd, args) => {
        calls.push([cmd, args]);
        if (cmd === 'list_library') {
          return {
            entries: [libraryEntry, { ...libraryEntry, id: 'no-preview', label: 'No preview', previewPath: null, gps: null, lens: null, tags: [] }],
            problems: [{ id: 'broken-entry', message: 'profile digest mismatch' }],
          };
        }
        if (cmd === 'apply_batch') {
          return {
            total: 1,
            succeeded: ['001.jpg'],
            skipped: [],
            failed: [],
            warnings: [{ file: '001.jpg', warning: 'camera mismatch allowed for library calibration: profile SONY ILCE-7M3, image NIKON Z6' }],
            metadata: [{ file: '001.jpg', copied: [], skipped: [] }],
          };
        }
        throw new Error(`Unhandled mock command: ${cmd}`);
      },
      transformCallback: (fn) => fn,
      unregisterCallback: () => undefined,
      convertFileSrc: (path) => `asset://${path}`,
    },
  };
  const tauri = await import('../src/tauri.ts');
  const interaction = await import('../src/interaction.ts');

  const listing = await tauri.backend.listLibrary('C:/library');
  assert.deepEqual(calls[0], ['list_library', { libraryPath: 'C:/library' }]);
  assert.deepEqual(listing.problems, [{ id: 'broken-entry', message: 'profile digest mismatch' }]);
  assert.equal(listing.entries.length, 2);
  const [entry, bare] = listing.entries;
  assert.equal(entry.previewUrl, 'asset://C:/library/sony-a7-studio-0396277a/preview.png');
  assert.equal('previewPath' in entry, false);
  assert.equal(bare.previewUrl, null);

  const card = interaction.libraryCardModel(entry);
  assert.equal(card.title, 'Studio A7');
  assert.equal(card.camera, 'SONY ILCE-7M3');
  assert.equal(card.lens, 'FE 35mm F1.8');
  assert.equal(card.capturedAt, '2026-10-03T18:33:14');
  assert.equal(card.gps, '52.5200, 13.4050 · 34 m');
  assert.deepEqual(card.badges, [{ label: 'PASSED', tone: 'ok' }]);
  assert.equal(card.deltaE, 'ΔE mean 0.84 / max 2.14');
  assert.equal(card.chartRevision, 'classic-from-november2014');
  assert.equal(card.decoder, 'rawler-ahd 0.7.1');
  assert.deepEqual(card.tags, ['studio', 'strobe']);
  assert.equal(interaction.libraryCardModel(bare).gps, null);

  // A loaded NIKON reference differs from the SONY calibration: notice, but not a block.
  assert.equal(interaction.libraryCameraMismatch(entry, { make: 'NIKON', model: 'Z6' }), true);
  assert.equal(interaction.libraryCameraMismatch(entry, { make: 'sony', model: 'ilce-7m3' }), false);
  assert.equal(interaction.libraryCameraMismatch(entry, null), false);

  // Use for batch: the library profile wins over a derived one and allows mismatch.
  const choice = interaction.chooseBatchProfile('derived.cbprofile.json', entry);
  assert.deepEqual(choice, { profilePath: entry.profilePath, allowMismatch: true, source: 'library' });
  assert.deepEqual(interaction.chooseBatchProfile('derived.cbprofile.json', null), {
    profilePath: 'derived.cbprofile.json',
    allowMismatch: false,
    source: 'derived',
  });
  assert.equal(interaction.chooseBatchProfile(null, null), null);

  const options = { overwrite: false, space: 'srgb', format: 'tiff', quality: 95, sampling: '444', includeXmpIptc: false, stripGps: true };
  const summary = await tauri.backend.applyBatch('unused', 'C:/in', 'C:/out', options, false);
  assert.deepEqual(calls[1], ['apply_batch', {
    profilePath: 'unused', inputPath: 'C:/in', outputPath: 'C:/out', exportOptions: options, allowMismatch: false,
  }]);
  const librarySummary = await tauri.backend.applyBatch(choice.profilePath, 'C:/in', 'C:/out', options, choice.allowMismatch);
  assert.deepEqual(calls[2], ['apply_batch', {
    profilePath: entry.profilePath, inputPath: 'C:/in', outputPath: 'C:/out', exportOptions: options, allowMismatch: true,
  }]);
  assert.equal(summary.warnings.length, 1);
  assert.match(librarySummary.warnings[0].warning, /^camera mismatch allowed for library calibration/);
});

test('library: save sends camelCase args and returns the entry with a preview URL; tags are parsed', async () => {
  const calls = [];
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (cmd, args) => {
        calls.push([cmd, args]);
        if (cmd === 'save_to_library') return libraryEntry;
        throw new Error(`Unhandled mock command: ${cmd}`);
      },
      transformCallback: (fn) => fn,
      unregisterCallback: () => undefined,
      convertFileSrc: (path) => `asset://${path}`,
    },
  };
  const tauri = await import('../src/tauri.ts');
  const interaction = await import('../src/interaction.ts');

  assert.deepEqual(interaction.parseTags(' studio, strobe ,,Studio, 5600K '), ['studio', 'strobe', '5600K']);
  const request = {
    libraryPath: 'C:/library',
    profilePath: 'colorbalance.cbprofile.json',
    referencePath: 'C:/images/ref.dng',
    label: 'Studio A7',
    notes: 'Strobe',
    tags: interaction.parseTags('studio, strobe'),
    includeGps: false,
  };
  const saved = await tauri.backend.saveToLibrary(request);
  assert.deepEqual(calls, [['save_to_library', request]]);
  assert.equal(saved.id, libraryEntry.id);
  assert.equal(saved.previewUrl, 'asset://C:/library/sony-a7-studio-0396277a/preview.png');

  // Storage is best-effort: whatever the runtime provides, reading and writing the folder never throws.
  assert.equal(typeof interaction.readStoredLibraryPath(), 'string');
  assert.doesNotThrow(() => interaction.storeLibraryPath(''));
});
