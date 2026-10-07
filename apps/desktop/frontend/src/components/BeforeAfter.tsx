import React, { useEffect, useRef, useState } from 'react';
import { TransformComponent, TransformWrapper, type ReactZoomPanPinchContentRef } from 'react-zoom-pan-pinch';
import { Minus, Plus, X } from 'lucide-react';

interface Props {
  /** Backend preview URL for the decoded image. */
  beforeSrc: string;
  /** Backend preview URL for the corrected image. */
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
  const [view, setView] = useState({ scale: 1, positionX: 0, positionY: 0 });
  const [backdrop, setBackdrop] = useState<Backdrop>('dark');
  const [peek, setPeek] = useState(false);
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [stage, setStage] = useState({ w: 0, h: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const controls = useRef<(ReactZoomPanPinchContentRef | null)[]>([]);

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
  const pixelScale = natural && fitW > 0 ? (view.scale * fitW) / natural.w : 0;

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


  const zoomBy = (factor: number) => {
    const control = controls.current[0];
    if (control) void control.centerView(clamp(control.state.scale * factor, 1, maxZoom), 0);
  };

  const fitView = () => {
    controls.current.forEach((control) => {
      if (control) void control.setTransform(originX, originY, 1, 0);
    });
  };

  const resetView = () => {
    fitView();
    setSplit(50);
  };

  const zoomTo100 = () => {
    const control = controls.current[0];
    if (control) void control.centerView(clamp(1 / fit, 1, maxZoom), 0);
  };

  const onDoubleClick = (index: number, event: React.MouseEvent<HTMLDivElement>) => {
    const control = controls.current[index];
    if (!control) return;
    if (control.state.scale > 1) {
      resetView();
    } else {
      void control.zoomToPoint(clamp(Math.max(2, 1 / fit), 1, maxZoom), event.clientX, event.clientY, 0);
    }
  };

  const onTransform = (index: number, state: { scale: number; positionX: number; positionY: number }) => {
    setView((current) =>
      current.scale === state.scale && current.positionX === state.positionX && current.positionY === state.positionY
        ? current
        : state,
    );
    const other = controls.current[1 - index];
    if (shown === 'side' && other &&
      (other.state.scale !== state.scale || other.state.positionX !== state.positionX || other.state.positionY !== state.positionY)) {
      void other.setTransform(state.positionX, state.positionY, state.scale, 0);
    }
  };

  // Each side pane gets a cursor-relative wheel and pinch surface; images stay outside the transformed node.
  const pane = (children: React.ReactNode, index = 0) => (
    <div key={index} data-pane className="relative overflow-hidden" style={{ width: areaW, height: areaH }}>
      <TransformWrapper
        key={`${areaW}-${areaH}-${fitW}-${fitH}`}
        ref={(control) => { controls.current[index] = control; }}
        minScale={1}
        maxScale={maxZoom}
        initialScale={clamp(view.scale, 1, maxZoom)}
        initialPositionX={originX}
        initialPositionY={originY}
        limitToBounds
        centerZoomedOut
        centerOnInit
        wheel={{ step: 0.0015 }}
        panning={{ disabled: view.scale <= 1, velocityDisabled: true }}
        doubleClick={{ disabled: true }}
        zoomAnimation={{ disabled: true }}
        onTransform={(_, state) => onTransform(index, state)}
      >
        <TransformComponent
          wrapperClass="!absolute !inset-0 !w-full !h-full"
          contentStyle={{ width: fitW, height: fitH }}
          wrapperProps={{ onPointerDown: () => rootRef.current?.focus(), onDoubleClick: (event) => onDoubleClick(index, event) }}
        >
          <div style={{ width: fitW, height: fitH }} />
        </TransformComponent>
      </TransformWrapper>
      {children}
    </div>
  );

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
  const transform = `translate(${view.positionX}px, ${view.positionY}px) scale(${view.scale})`;

  const layer = (src: string, label: string, testId: string, clip?: string) => (
    <div className="absolute inset-0 pointer-events-none" style={clip ? { clipPath: clip } : undefined}>
      <div className="absolute" style={{ width: fitW, height: fitH, transform, transformOrigin: '0 0' }}>
        <img src={src} alt={label} draggable={false} className="block w-full h-full" style={imageStyle} data-testid={testId} />
      </div>
    </div>
  );

  const tag = (text: string, position: string) => (
    <span className={`absolute px-1 text-[9px] bg-black/60 text-[var(--bb-sand)] pointer-events-none ${position}`}>
      {text}
    </span>
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
            0,
          )}
          {pane(
            <>
              {layer(afterSrc, 'After', 'after-image')}
              {tag('AFTER', 'top-1 left-1')}
            </>,
            1,
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
          role="slider"
          tabIndex={0}
          aria-label="Before and after split"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(split)}
          aria-orientation={vertical ? 'horizontal' : 'vertical'}
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
            onClick={fitView}
            className={chip(view.scale === 1)}
            data-testid="zoom-fit"
          >
            FIT
          </button>
          <button
            type="button"
            tabIndex={-1}
            title="One display pixel per preview pixel"
            onClick={zoomTo100}
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
        style={{ background: BACKDROP[backdrop], cursor: view.scale > 1 ? 'grab' : 'default' }}
        data-testid="compare-frame"
      >
        {renderStage()}
      </div>

      <div className="flex flex-wrap justify-between gap-x-4 px-2 py-1 border-t border-[var(--bb-border)] text-[9px] text-[var(--bb-smoke)]">
        <span data-testid="compare-status">
          {shown.toUpperCase()}
          {shown === 'split' ? ` ${Math.round(split)}%` : ''} · {Math.round(pixelScale * 100)}%
        </span>
        <span>SCROLL/PINCH ZOOM · DRAG PAN · DOUBLE-CLICK ZOOM · SPACE HOLD BEFORE · 1-4 VIEW · ARROWS SPLIT · 0 RESET</span>
      </div>
    </div>
  );
};
