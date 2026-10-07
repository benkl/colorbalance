import React from 'react';
import { AlertCircle, FolderOpen, ImageOff, RefreshCw } from 'lucide-react';
import type { LibraryEntryView, LibraryListingView } from '../types';
import { libraryCameraMismatch, libraryCardModel } from '../interaction';
import type { QualityBadge } from '../interaction';

const TONE_CLASS: Record<QualityBadge['tone'], string> = {
  ok: 'text-[var(--bb-gold)] border-[var(--bb-gold)]',
  warn: 'text-[var(--bb-orange)] border-[var(--bb-orange)]',
  bad: 'text-[var(--bb-crimson)] border-[var(--bb-crimson)]',
};

const Badges: React.FC<{ badges: QualityBadge[] }> = ({ badges }) => (
  <div className="flex flex-wrap gap-1">
    {badges.map((badge) => (
      <span
        key={badge.label}
        data-testid="library-badge"
        className={`px-1.5 py-0.5 border text-[9px] font-bold tracking-wider ${TONE_CLASS[badge.tone]}`}
      >
        {badge.label}
      </span>
    ))}
  </div>
);

const TagList: React.FC<{ tags: string[] }> = ({ tags }) =>
  tags.length > 0 ? (
    <div className="flex flex-wrap gap-1" data-testid="library-tags">
      {tags.map((tag) => (
        <span key={tag} className="px-1.5 py-0.5 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] text-[9px] text-[var(--bb-amber)]">
          {tag}
        </span>
      ))}
    </div>
  ) : null;

interface GalleryProps {
  listing: LibraryListingView | null;
  libraryPath: string;
  loading: boolean;
  selectedId: string | null;
  activeId: string | null;
  onSelect: (id: string) => void;
}

/** Card grid for the viewport: one card per verified library entry. */
export const LibraryGallery: React.FC<GalleryProps> = ({ listing, libraryPath, loading, selectedId, activeId, onSelect }) => (
  <div className="flex-1 min-h-0 overflow-y-auto p-4" data-testid="library-gallery">
    {!libraryPath && (
      <div className="text-[10px] text-[var(--bb-smoke)]">Choose a library folder in the panel on the right.</div>
    )}
    {libraryPath && loading && !listing && <div className="text-[10px] text-[var(--bb-smoke)]">Scanning library…</div>}
    {listing && listing.entries.length === 0 && (
      <div className="text-[10px] text-[var(--bb-smoke)]" data-testid="library-empty">
        No usable entries in this folder. Save one from the VALIDATE tab after deriving a profile.
      </div>
    )}
    {listing && listing.entries.length > 0 && (
      <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))' }}>
        {listing.entries.map((entry) => {
          const card = libraryCardModel(entry);
          const selected = entry.id === selectedId;
          return (
            <button
              key={entry.id}
              type="button"
              data-testid="library-card"
              aria-pressed={selected}
              onClick={() => onSelect(entry.id)}
              className={`text-left flex flex-col bg-[var(--bb-panel)] border cursor-pointer transition-all ${
                selected
                  ? 'border-[var(--bb-gold)] shadow-[0_0_8px_rgba(245,185,49,0.2)]'
                  : 'border-[var(--bb-border)] hover:border-[var(--bb-smoke)]'
              }`}
            >
              <div className="h-[150px] bg-[var(--bb-vacuum)] flex items-center justify-center overflow-hidden border-b border-[var(--bb-border)]">
                {entry.previewUrl ? (
                  <img src={entry.previewUrl} alt={`Preview of ${card.title}`} className="max-w-full max-h-full object-contain" />
                ) : (
                  <div className="flex flex-col items-center gap-1 text-[var(--bb-smoke)]" data-testid="library-no-preview">
                    <ImageOff className="w-5 h-5" />
                    <span className="text-[9px]">NO PREVIEW</span>
                  </div>
                )}
              </div>
              <div className="p-2.5 space-y-1.5 text-[10px] min-w-0">
                <div className="flex items-start justify-between gap-2">
                  <div className="text-[11px] font-bold text-[var(--bb-gold)] break-words min-w-0" data-testid="library-card-label">
                    {card.title}
                  </div>
                  {entry.id === activeId && (
                    <span className="shrink-0 px-1.5 py-0.5 border border-[var(--bb-amber)] text-[9px] font-bold text-[var(--bb-amber)]">ACTIVE</span>
                  )}
                </div>
                <div className="text-[var(--bb-sand)] break-words">{card.camera}</div>
                {card.lens && <div className="text-[var(--bb-smoke)] break-words">{card.lens}</div>}
                {card.capturedAt && <div className="text-[var(--bb-smoke)]">{card.capturedAt}</div>}
                {card.gps && <div className="text-[var(--bb-smoke)]">GPS {card.gps}</div>}
                <Badges badges={card.badges} />
                <div className="text-[var(--bb-sand)]">{card.deltaE}</div>
                <div className="text-[var(--bb-smoke)] break-words">{card.chartRevision}</div>
                <div className="text-[var(--bb-smoke)] break-words">{card.decoder}</div>
                <TagList tags={card.tags} />
              </div>
            </button>
          );
        })}
      </div>
    )}
  </div>
);

/** Export viewport for an activated library profile: what the entry holds, since there is no preview to render. */
export const ActiveProfile: React.FC<{
  entry: LibraryEntryView;
  loadedCamera: { make: string; model: string } | null;
}> = ({ entry, loadedCamera }) => {
  const card = libraryCardModel(entry);
  return (
    <div className="flex-1 min-h-0 overflow-y-auto p-4" data-testid="active-profile">
      <div className="max-w-3xl mx-auto bg-[var(--bb-panel)] border border-[var(--bb-border)] flex flex-col md:flex-row">
        <div className="md:w-[260px] shrink-0 min-h-[160px] bg-[var(--bb-vacuum)] flex items-center justify-center overflow-hidden border-b md:border-b-0 md:border-r border-[var(--bb-border)]">
          {entry.previewUrl ? (
            <img src={entry.previewUrl} alt={`Reference of ${card.title}`} className="max-w-full max-h-[260px] object-contain" />
          ) : (
            <div className="flex flex-col items-center gap-1 text-[var(--bb-smoke)]">
              <ImageOff className="w-5 h-5" />
              <span className="text-[9px]">NO PREVIEW</span>
            </div>
          )}
        </div>
        <div className="flex-1 min-w-0 p-3 space-y-2 text-[10px]">
          <div className="text-[9px] font-bold tracking-wider text-[var(--bb-amber)]">ACTIVE LIBRARY PROFILE</div>
          <div className="text-[13px] font-bold text-[var(--bb-gold)] break-words" data-testid="active-profile-label">{card.title}</div>
          <div className="whitespace-pre-wrap break-words text-[var(--bb-sand)]" data-testid="active-profile-notes">
            {entry.notes || <span className="text-[var(--bb-smoke)]">No notes.</span>}
          </div>
          <TagList tags={card.tags} />
          <Badges badges={card.badges} />
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[10px]">
            <dt className="text-[var(--bb-smoke)]">CAMERA</dt>
            <dd className="text-[var(--bb-sand)] break-words">{card.camera}</dd>
            {card.lens && (<><dt className="text-[var(--bb-smoke)]">LENS</dt><dd className="text-[var(--bb-sand)] break-words">{card.lens}</dd></>)}
            {card.capturedAt && (<><dt className="text-[var(--bb-smoke)]">CAPTURED</dt><dd className="text-[var(--bb-sand)]">{card.capturedAt}</dd></>)}
            {card.gps && (<><dt className="text-[var(--bb-smoke)]">GPS</dt><dd className="text-[var(--bb-sand)]">{card.gps}</dd></>)}
            <dt className="text-[var(--bb-smoke)]">FIT</dt>
            <dd className="text-[var(--bb-sand)]">{card.deltaE} over {entry.patchCount} patches</dd>
            <dt className="text-[var(--bb-smoke)]">CHART</dt>
            <dd className="text-[var(--bb-sand)] break-words">{card.chartRevision}</dd>
            <dt className="text-[var(--bb-smoke)]">DECODER</dt>
            <dd className="text-[var(--bb-sand)] break-words">{card.decoder}</dd>
            <dt className="text-[var(--bb-smoke)]">DIGEST</dt>
            <dd className="text-[var(--bb-sand)] break-all">{entry.digest}</dd>
            <dt className="text-[var(--bb-smoke)]">PROFILE</dt>
            <dd className="text-[var(--bb-sand)] break-all">{entry.profilePath}</dd>
          </dl>
          <CameraMismatchNotice entry={entry} loadedCamera={loadedCamera} />
          <p className="text-[var(--bb-smoke)]">
            No preview is rendered for a Library profile, because its reference may come from another camera. Apply it to an image or folder to see a result here.
          </p>
        </div>
      </div>
    </div>
  );
};

interface PanelProps {
  listing: LibraryListingView | null;
  libraryPath: string;
  loading: boolean;
  error: string;
  selected: LibraryEntryView | null;
  active: LibraryEntryView | null;
  /** Camera of the loaded reference, when inspected. Used only for the mismatch notice. */
  loadedCamera: { make: string; model: string } | null;
  onLibraryPathChange: (path: string) => void;
  onChoose: () => void;
  onRefresh: () => void;
  onUse: (entry: LibraryEntryView) => void;
}

/** Inspector side: folder, refresh, problems, and the selected entry's notes and details. */
export const LibraryPanel: React.FC<PanelProps> = ({
  listing,
  libraryPath,
  loading,
  error,
  selected,
  active,
  loadedCamera,
  onLibraryPathChange,
  onChoose,
  onRefresh,
  onUse,
}) => (
  <div className="space-y-3.5" data-testid="library-panel">
    <div className="border-b border-[var(--bb-border)] pb-2">
      <h2 className="text-[11px] font-bold text-[var(--bb-gold)] tracking-wider">CALIBRATION LIBRARY</h2>
      <p className="text-[10px] text-[var(--bb-smoke)]">
        Saved calibrations in one folder. Each entry is verified by its profile digest when the folder is scanned.
      </p>
    </div>

    <div className="space-y-1">
      <label htmlFor="library-path" className="text-[9px] text-[var(--bb-smoke)] font-bold tracking-wider">LIBRARY FOLDER</label>
      <div className="flex gap-1.5">
        <input
          id="library-path"
          type="text"
          value={libraryPath}
          onChange={(e) => onLibraryPathChange(e.target.value)}
          placeholder="LIBRARY DIRECTORY…"
          className="flex-1 bg-[var(--bb-vacuum)] border border-[var(--bb-border)] px-2.5 py-1 text-[11px] text-[var(--bb-sand)] focus:border-[var(--bb-gold)] outline-none min-w-0"
        />
        <button type="button" className="ui-btn ui-btn-secondary shrink-0" onClick={onChoose} data-testid="library-choose">
          <FolderOpen className="w-3 h-3" /> CHOOSE
        </button>
      </div>
      <button
        type="button"
        className="ui-btn ui-btn-secondary w-full"
        onClick={onRefresh}
        disabled={!libraryPath || loading}
        data-testid="library-refresh"
      >
        <RefreshCw className="w-3 h-3" /> {loading ? 'SCANNING…' : 'REFRESH'}
      </button>
    </div>

    {error && (
      <div className="p-2.5 bg-[var(--bb-ember-dark)]/40 border border-[var(--bb-crimson)] text-[10px] text-[var(--bb-sand)] break-words" data-testid="library-error">
        {error}
      </div>
    )}

    {listing && listing.problems.length > 0 && (
      <div className="p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-orange)] space-y-1" data-testid="library-problems">
        <div className="text-[10px] font-bold text-[var(--bb-orange)] tracking-wider flex items-center gap-1.5">
          <AlertCircle className="w-3 h-3" /> PROBLEMS ({listing.problems.length})
        </div>
        <ul className="space-y-1 text-[9px] text-[var(--bb-sand)] max-h-40 overflow-y-auto">
          {listing.problems.map((problem) => (
            <li key={problem.id}>
              <span className="text-[var(--bb-gold)] break-all">{problem.id}</span>
              <div className="text-[var(--bb-orange)] break-words">⚠ {problem.message}</div>
            </li>
          ))}
        </ul>
        <div className="text-[9px] text-[var(--bb-smoke)]">These folders are not listed as usable calibrations.</div>
      </div>
    )}

    {selected ? (
      <div className="p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-border)] space-y-2 text-[10px]" data-testid="library-detail">
        <div className="text-[11px] font-bold text-[var(--bb-gold)] break-words">{selected.label}</div>
        <div className="whitespace-pre-wrap break-words text-[var(--bb-sand)]" data-testid="library-notes">
          {selected.notes || <span className="text-[var(--bb-smoke)]">No notes.</span>}
        </div>
        <TagList tags={selected.tags} />
        <dl className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-[9px]">
          <dt className="text-[var(--bb-smoke)]">PATCHES</dt>
          <dd className="text-[var(--bb-sand)]">{selected.patchCount}</dd>
          <dt className="text-[var(--bb-smoke)]">DIGEST</dt>
          <dd className="text-[var(--bb-sand)] break-all">{selected.digest}</dd>
          <dt className="text-[var(--bb-smoke)]">PROFILE</dt>
          <dd className="text-[var(--bb-sand)] break-all">{selected.profilePath}</dd>
        </dl>
        <CameraMismatchNotice entry={selected} loadedCamera={loadedCamera} />
        <button
          type="button"
          className="ui-btn ui-btn-primary w-full h-9"
          onClick={() => onUse(selected)}
          data-testid="library-use"
        >
          {active?.id === selected.id ? 'ACTIVE PROFILE' : 'USE THIS PROFILE'}
        </button>
      </div>
    ) : (
      <div className="p-2.5 bg-[var(--bb-panel)] border border-[var(--bb-border)] text-[10px] text-[var(--bb-smoke)]">
        Select a card to see its notes, then use it as the active profile on the Export tab.
      </div>
    )}
  </div>
);

/** Shown when a library calibration was made for a different camera than the loaded reference. */
export const CameraMismatchNotice: React.FC<{
  entry: Pick<LibraryEntryView, 'cameraMake' | 'cameraModel'>;
  loadedCamera: { make: string; model: string } | null;
}> = ({ entry, loadedCamera }) =>
  loadedCamera && libraryCameraMismatch(entry, loadedCamera) ? (
    <div className="p-2 border border-[var(--bb-orange)] bg-[var(--bb-ember-dark)]/30 text-[9px] text-[var(--bb-sand)]" data-testid="library-mismatch">
      <span className="font-bold text-[var(--bb-orange)]">⚠ CAMERA MISMATCH</span>: this calibration is for {entry.cameraMake} {entry.cameraModel}, the
      loaded reference is {loadedCamera.make} {loadedCamera.model}. A batch continues with a warning per file; a single image is refused.
    </div>
  ) : null;
