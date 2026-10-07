import React, { useEffect, useRef, useState } from 'react';
import { Minus, Plus, X } from 'lucide-react';

interface Props {
  /** `data:image/png` of the image as decoded. Rendered by the backend. */
  beforeSrc: string;
  /** `data:image/png` of the corrected image. Rendered by the backend. */
  afterSrc: string;
  onClose: () => void;
}

type Mode = 'split' | 'side' | 'before' | 'after';
type Orientation = 'vertical' | 'horizontal';
type Backdrop = 'dark' | 'gray' | 'light';

const MODES: { id: Mode; label: string; key: string; testId: string }[] = [
  { id: 'split', label: 'SPLIT', key: '1', testId: 'compare-slider' },
  { id: 'side', label: 'SIDE BY SIDE', key: '2', testId: 'compare-side' },
  { id: 'before', label: 'BEFORE', key: '3', testId: 'compare-before' },
  { id: 'after', label: 'AFTER', key: '4', testId: 'compare-after' },
];

/** Neutral surrounds for judging color; the middle one is a 50% gray. */
const BACKDROP: Record<Backdrop, string> = { dark: '#0a0a0a', gray: '#808080', light: '#f2f2f2' };

const GAP = 8;
/** Pixels of the image that must stay inside the viewport while panning. */
const PAN_MARGIN = 48;
const ZOOM_STEP = 1.25;
/** Highest zoom is this many display pixels per preview pixel. */
const MAX_PIXEL_SCALE = 8;

const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

const chip = (active: boolean) =>
  `px-2 py-0.5 border text-[10px] ${
    active
      ? 'border-[var(--bb-gold)] text-[var(--bb-gold)]'
      : 'border-[var(--bb-border)] text-[var(--bb-smoke)] hover:text-[var(--bb-sand)]'
  }`;

/**
 * Viewer for two backend-rendered previews of the same image. Views: split
 * reveal, side by side, or either image alone. Zoom and pan are shared by
 * both images so the same pixels stay aligned. Display only: no pixel values
 * are read or changed here.
 */
export const BeforeAfter: React.FC<Props> = ({ beforeSrc, afterSrc, onClose }) => {
  const [mode, setMode] = useState<Mode>('split');
  const [orientation, setOrientation] = useState<Orientation>('vertical');
  const [split, setSplit] = useState(50);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [backdrop, setBackdrop] = useState<Backdrop>('dark');
  const [peek, setPeek] = useState(false);
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [stage, setStage] = useState({ w: 0, h: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; pan: { x: number; y: number } } | null>(null);

  // Holding the peek key or button shows the original, whatever the view.
  const shown: Mode = peek ? 'before' : mode;
  const panes = shown === 'side' ? 2 : 1;
  const areaW = panes === 2 ? (stage.w - GAP) / 2 : stage.w;
  const areaH = stage.h;
  const fit = natural && areaW > 0 && areaH > 0 ? Math.min(areaW / natural.w, areaH / natural.h) : 0;
  const fitW = natural ? natural.w * fit : 0;
  const fitH = natural ? natural.h * fit : 0;
  const maxZoom = natural && fitW > 0 ? Math.max(1, (MAX_PIXEL_SCALE * natural.w) / fitW) : 1;
  const originX = (areaW - fitW) / 2;
  const originY = (areaH - fitH) / 2;
  const pixelScale = natural && fitW > 0 ? (zoom * fitW) / natural.w : 0;

  useEffect(() => {
    let live = true;
    const probe = new Image();
    probe.onload = () => {
      if (live) setNatural({ w: probe.naturalWidth, h: probe.naturalHeight });
    };
    probe.src = afterSrc;
    return () => {
      live = false;
    };
  }, [afterSrc]);

  useEffect(() => {
    const node = stageRef.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) =>
      setStage({ w: entry.contentRect.width, h: entry.contentRect.height }),
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    rootRef.current?.focus();
  }, []);

  const clampPan = (next: { x: number; y: number }, z: number) => {
    if (z <= 1) return { x: 0, y: 0 };
    return {
      x: clamp(next.x, -(originX + fitW * z - PAN_MARGIN), areaW - originX - PAN_MARGIN),
      y: clamp(next.y, -(originY + fitH * z - PAN_MARGIN), areaH - originY - PAN_MARGIN),
    };
  };

  /** Zoom keeping the image point under (px, py) fixed; coordinates are relative to one pane. */
  const zoomAt = (target: number, px: number, py: number) => {
    const next = clamp(target, 1, maxZoom);
    if (next <= 1) {
      setZoom(1);
      setPan({ x: 0, y: 0 });
      return;
    }
    const contentX = (px - originX - pan.x) / zoom;
    const contentY = (py - originY - pan.y) / zoom;
    setZoom(next);
    setPan(clampPan({ x: px - originX - contentX * next, y: py - originY - contentY * next }, next));
  };

  const zoomBy = (factor: number) => zoomAt(zoom * factor, areaW / 2, areaH / 2);

  const resetView = () => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
    setSplit(50);
  };

  // Native listener: React registers wheel passively, which forbids preventDefault.
  useEffect(() => {
    const node = stageRef.current;
    if (!node) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const pane = (event.target as Element).closest('[data-pane]') as HTMLElement | null;
      const rect = (pane ?? node).getBoundingClientRect();
      zoomAt(zoom * Math.exp(-event.deltaY * 0.0015), event.clientX - rect.left, event.clientY - rect.top);
    };
    node.addEventListener('wheel', onWheel, { passive: false });
    return () => node.removeEventListener('wheel', onWheel);
  });

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const stepKey = orientation === 'vertical' ? ['ArrowLeft', 'ArrowRight'] : ['ArrowUp', 'ArrowDown'];
    if (event.key === ' ') {
      event.preventDefault();
      setPeek(true);
    } else if (['1', '2', '3', '4'].includes(event.key)) {
      setMode(MODES[Number(event.key) - 1].id);
    } else if (event.key === '+' || event.key === '=') {
      zoomBy(ZOOM_STEP);
    } else if (event.key === '-') {
      zoomBy(1 / ZOOM_STEP);
    } else if (event.key === '0') {
      resetView();
    } else if (mode === 'split' && stepKey.includes(event.key)) {
      event.preventDefault();
      setSplit((value) => clamp(value + (event.key === stepKey[0] ? -2 : 2), 0, 100));
    } else if (event.key === 'Escape') {
      onClose();
    }
  };

  const onStagePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    rootRef.current?.focus();
    if (event.button !== 0 || zoom <= 1) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, y: event.clientY, pan };
  };

  const onStagePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start) return;
    setPan(
      clampPan(
        { x: start.pan.x + event.clientX - start.x, y: start.pan.y + event.clientY - start.y },
        zoom,
      ),
    );
  };

  const onStageDoubleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!natural) return;
    if (zoom > 1) {
      resetView();
      return;
    }
    const pane = (event.target as Element).closest('[data-pane]') as HTMLElement | null;
    const rect = (pane ?? event.currentTarget).getBoundingClientRect();
    zoomAt(Math.max(2, natural.w / Math.max(fitW, 1)), event.clientX - rect.left, event.clientY - rect.top);
  };

  const moveSplit = (event: React.PointerEvent<HTMLDivElement>) => {
    const rect = stageRef.current?.getBoundingClientRect();
    if (!rect || rect.width === 0 || rect.height === 0) return;
    const fraction =
      orientation === 'vertical'
        ? (event.clientX - rect.left) / rect.width
        : (event.clientY - rect.top) / rect.height;
    setSplit(clamp(fraction * 100, 0, 100));
  };

  const imageStyle: React.CSSProperties = { imageRendering: pixelScale >= 2 ? 'pixelated' : 'auto' };
  const transform = `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`;

  const layer = (src: string, label: string, testId: string, clip?: string) => (
    <div className="absolute inset-0" style={clip ? { clipPath: clip } : undefined}>
      <div
        className="absolute"
        style={{ left: originX, top: originY, width: fitW, height: fitH, transform, transformOrigin: '0 0' }}
      >
        <img src={src} alt={label} draggable={false} className="block w-full h-full" style={imageStyle} data-testid={testId} />
      </div>
    </div>
  );

  const tag = (text: string, position: string) => (
    <span className={`absolute px-1 text-[9px] bg-black/60 text-[var(--bb-sand)] pointer-events-none ${position}`}>
      {text}
    </span>
  );

  const pane = (children: React.ReactNode, key?: string) => (
    <div key={key} data-pane className="relative overflow-hidden" style={{ width: areaW, height: areaH }}>
      {children}
    </div>
  );

  const renderStage = () => {
    if (!natural || stage.w <= 0 || stage.h <= 0) return null;
    if (shown === 'side') {
      return (
        <div className="flex" style={{ gap: GAP, height: areaH }}>
          {pane(
            <>
              {layer(beforeSrc, 'Before', 'before-image')}
              {tag('BEFORE', 'top-1 left-1')}
            </>,
            'before',
          )}
          {pane(
            <>
              {layer(afterSrc, 'After', 'after-image')}
              {tag('AFTER', 'top-1 left-1')}
            </>,
            'after',
          )}
        </div>
      );
    }
    if (shown === 'before') {
      return pane(
        <>
          {layer(beforeSrc, 'Before', 'before-image')}
          {tag(peek ? 'BEFORE (HELD)' : 'BEFORE', 'top-1 left-1')}
        </>,
      );
    }
    if (shown === 'after') {
      return pane(
        <>
          {layer(afterSrc, 'After', 'after-image')}
          {tag('AFTER', 'top-1 left-1')}
        </>,
      );
    }
    const vertical = orientation === 'vertical';
    const clip = vertical ? `inset(0 ${100 - split}% 0 0)` : `inset(0 0 ${100 - split}% 0)`;
    return pane(
      <>
        {layer(afterSrc, 'After', 'after-image')}
        {layer(beforeSrc, 'Before', 'before-image', clip)}
        {tag('BEFORE', 'top-1 left-1')}
        {tag('AFTER', vertical ? 'top-1 right-1' : 'bottom-1 left-1')}
        <div
          className={`absolute flex items-center justify-center ${
            vertical ? 'top-0 bottom-0 w-6 -ml-3 cursor-ew-resize' : 'left-0 right-0 h-6 -mt-3 cursor-ns-resize'
          }`}
          style={vertical ? { left: `${split}%` } : { top: `${split}%` }}
          onPointerDown={(event) => {
            event.stopPropagation();
            event.currentTarget.setPointerCapture(event.pointerId);
            moveSplit(event);
          }}
          onPointerMove={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) moveSplit(event);
          }}
          data-testid="split-handle"
        >
          <div className={`absolute bg-[var(--bb-gold)] ${vertical ? 'top-0 bottom-0 w-px' : 'left-0 right-0 h-px'}`} />
          <div className="relative w-4 h-4 rounded-full border border-[var(--bb-gold)] bg-black/70" />
        </div>
      </>,
    );
  };

  return (
    <div
      ref={rootRef}
      tabIndex={0}
      className="h-full w-full flex flex-col bg-[var(--bb-vacuum)] font-mono outline-none"
      onKeyDown={onKeyDown}
      onKeyUp={(event) => {
        if (event.key === ' ') setPeek(false);
      }}
      onBlur={() => setPeek(false)}
      data-testid="before-after"
    >
      {/* Keep keyboard focus on the viewer when toolbar buttons are clicked. */}
      <div
        className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-2 py-1.5 border-b border-[var(--bb-border)] bg-[var(--bb-panel)]"
        onMouseDown={(event) => event.preventDefault()}
      >
        <span className="text-[10px] font-bold tracking-wider text-[var(--bb-gold)]">COMPARE</span>

        <div className="flex gap-1" role="group" aria-label="View">
          {MODES.map(({ id, label, key, testId }) => (
            <button
              key={id}
              type="button"
              tabIndex={-1}
              title={`${label} (${key})`}
              onClick={() => setMode(id)}
              className={chip(mode === id)}
              data-testid={testId}
            >
              {label}
            </button>
          ))}
        </div>

        {mode === 'split' && (
          <button
            type="button"
            tabIndex={-1}
            title="Direction of the split line"
            onClick={() => setOrientation((value) => (value === 'vertical' ? 'horizontal' : 'vertical'))}
            className={chip(false)}
            data-testid="split-orientation"
          >
            {orientation === 'vertical' ? 'LEFT | RIGHT' : 'TOP / BOTTOM'}
          </button>
        )}

        <div className="flex items-center gap-1" role="group" aria-label="Zoom">
          <button type="button" tabIndex={-1} title="Zoom out (-)" onClick={() => zoomBy(1 / ZOOM_STEP)} className={chip(false)} data-testid="zoom-out">
            <Minus className="w-3 h-3" />
          </button>
          <span className="min-w-[3.5rem] text-center text-[10px] text-[var(--bb-sand)]" data-testid="zoom-readout">
            {Math.round(pixelScale * 100)}%
          </span>
          <button type="button" tabIndex={-1} title="Zoom in (+)" onClick={() => zoomBy(ZOOM_STEP)} className={chip(false)} data-testid="zoom-in">
            <Plus className="w-3 h-3" />
          </button>
          <button
            type="button"
            tabIndex={-1}
            title="Fit the image in the viewport"
            onClick={() => {
              setZoom(1);
              setPan({ x: 0, y: 0 });
            }}
            className={chip(zoom === 1)}
            data-testid="zoom-fit"
          >
            FIT
          </button>
          <button
            type="button"
            tabIndex={-1}
            title="One display pixel per preview pixel"
            onClick={() => natural && zoomAt(natural.w / Math.max(fitW, 1), areaW / 2, areaH / 2)}
            className={chip(Math.abs(pixelScale - 1) < 0.005)}
            data-testid="zoom-100"
          >
            100%
          </button>
        </div>

        <div className="flex items-center gap-1" role="group" aria-label="Backdrop">
          {(['dark', 'gray', 'light'] as const).map((value) => (
            <button
              key={value}
              type="button"
              tabIndex={-1}
              title={`${value} backdrop`}
              onClick={() => setBackdrop(value)}
              className={`w-4 h-4 border ${backdrop === value ? 'border-[var(--bb-gold)]' : 'border-[var(--bb-border)]'}`}
              style={{ background: BACKDROP[value] }}
              data-testid={`compare-bg-${value}`}
            />
          ))}
        </div>

        <button
          type="button"
          tabIndex={-1}
          title="Hold to show the original (Space)"
          onPointerDown={() => setPeek(true)}
          onPointerUp={() => setPeek(false)}
          onPointerLeave={() => setPeek(false)}
          className={chip(peek)}
          data-testid="compare-peek"
        >
          HOLD: BEFORE
        </button>

        <button type="button" tabIndex={-1} title="Reset zoom, pan, and split (0)" onClick={resetView} className={chip(false)} data-testid="view-reset">
          RESET
        </button>

        <button
          type="button"
          tabIndex={-1}
          title="Close the comparison (Esc)"
          onClick={onClose}
          className={`${chip(false)} ml-auto flex items-center gap-1`}
          data-testid="close-compare"
        >
          <X className="w-3 h-3" /> CLOSE
        </button>
      </div>

      <div
        ref={stageRef}
        className="relative flex-1 min-h-0 overflow-hidden select-none touch-none"
        style={{ background: BACKDROP[backdrop], cursor: zoom > 1 ? 'grab' : 'default' }}
        onPointerDown={onStagePointerDown}
        onPointerMove={onStagePointerMove}
        onPointerUp={() => {
          drag.current = null;
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
        onDoubleClick={onStageDoubleClick}
        data-testid="compare-frame"
      >
        {renderStage()}
      </div>

      <div className="flex flex-wrap justify-between gap-x-4 px-2 py-1 border-t border-[var(--bb-border)] text-[9px] text-[var(--bb-smoke)]">
        <span data-testid="compare-status">
          {shown.toUpperCase()}
          {shown === 'split' ? ` ${Math.round(split)}%` : ''} · {Math.round(pixelScale * 100)}%
        </span>
        <span>SCROLL ZOOM · DRAG PAN · DOUBLE-CLICK ZOOM · SPACE HOLD BEFORE · 1-4 VIEW · 0 RESET</span>
      </div>
    </div>
  );
};
