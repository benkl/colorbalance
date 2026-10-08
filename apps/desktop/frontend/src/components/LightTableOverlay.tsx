import React, { useRef, useState, useLayoutEffect, useCallback } from 'react';
import type { Point, ChartQuad } from '../types';

interface Props {
  imageSrc?: string;
  imageWidth?: number;
  imageHeight?: number;
  quad: ChartQuad;
  onQuadChange: (quad: ChartQuad) => void;
  onQuadInteractionStart?: () => void;
  onBrowse?: () => void;
  disabled?: boolean;
}

export const LightTableOverlay: React.FC<Props> = ({
  imageSrc,
  imageWidth = 480,
  imageHeight = 320,
  quad,
  onQuadChange,
  onQuadInteractionStart,
  onBrowse,
  disabled = false,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [activeCorner, setActiveCorner] = useState<number | null>(null);

  // Exact letterbox placement of the image inside the container
  const [box, setBox] = useState<{ x: number; y: number; w: number; h: number }>({
    x: 0,
    y: 0,
    w: 0,
    h: 0,
  });

  const updateLayout = useCallback(() => {
    if (!containerRef.current) return;
    const { clientWidth: cw, clientHeight: ch } = containerRef.current;
    if (cw === 0 || ch === 0) return;

    const imgAspect = imageWidth / imageHeight;
    const contAspect = cw / ch;

    let w: number;
    let h: number;
    let x: number;
    let y: number;

    if (contAspect > imgAspect) {
      // Container is wider than image: height fits, letterbox sides
      h = ch;
      w = h * imgAspect;
      x = (cw - w) / 2;
      y = 0;
    } else {
      // Container is taller than image: width fits, letterbox top/bottom
      w = cw;
      h = w / imgAspect;
      x = 0;
      y = (ch - h) / 2;
    }

    setBox({ x, y, w, h });
  }, [imageWidth, imageHeight]);

  useLayoutEffect(() => {
    updateLayout();
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(updateLayout);
    ro.observe(el);
    return () => ro.disconnect();
  }, [updateLayout]);

  const cornerLabels = ['TL', 'TR', 'BR', 'BL'];

  // Offset (container px) from the pointer to the marker centre at grab time, so
  // the marker follows the pointer 1:1 instead of snapping its centre to it.
  const grabOffset = useRef<{ dx: number; dy: number }>({ dx: 0, dy: 0 });

  const handlePointerDown = (index: number) => (e: React.PointerEvent) => {
    if (disabled || !containerRef.current) return;
    e.preventDefault();
    e.stopPropagation();
    onQuadInteractionStart?.();
    const rect = containerRef.current.getBoundingClientRect();
    const centre = toScreen(quad[index]);
    grabOffset.current = {
      dx: centre.x - (e.clientX - rect.left),
      dy: centre.y - (e.clientY - rect.top),
    };
    // Capture on the stage so move/up keep arriving however fast the pointer travels.
    containerRef.current.setPointerCapture(e.pointerId);
    setActiveCorner(index);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    if (activeCorner === null || disabled || box.w === 0 || box.h === 0 || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();

    // Where the marker centre should be, in container pixels.
    const clientX = e.clientX - rect.left + grabOffset.current.dx;
    const clientY = e.clientY - rect.top + grabOffset.current.dy;

    // Convert to image coordinates [0, imageWidth] x [0, imageHeight]
    const normX = Math.max(0, Math.min(1, (clientX - box.x) / box.w));
    const normY = Math.max(0, Math.min(1, (clientY - box.y) / box.h));

    const imgX = normX * imageWidth;
    const imgY = normY * imageHeight;

    const newQuad = [...quad] as ChartQuad;
    newQuad[activeCorner] = { x: Math.round(imgX * 10) / 10, y: Math.round(imgY * 10) / 10 };
    onQuadChange(newQuad);
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    if (activeCorner === null) return;
    try {
      containerRef.current?.releasePointerCapture(e.pointerId);
    } catch {
      // Capture was already released (e.g. by pointercancel).
    }
    setActiveCorner(null);
  };

  // Convert image coordinates into container pixels for SVG rendering
  const toScreen = (p: Point): Point => ({
    x: box.x + (p.x / imageWidth) * box.w,
    y: box.y + (p.y / imageHeight) * box.h,
  });

  // Calculate bilinear 24-patch sample boxes in container screen space
  const gridPolygons: string[] = [];
  if (box.w > 0 && box.h > 0) {
    for (let r = 0; r < 4; r++) {
      for (let c = 0; c < 6; c++) {
        const u0 = c / 6;
        const u1 = (c + 1) / 6;
        const v0 = r / 4;
        const v1 = (r + 1) / 4;
        const du = u1 - u0;
        const dv = v1 - v0;

        const bilinear = (u: number, v: number): Point => {
          const top = {
            x: (1 - u) * quad[0].x + u * quad[1].x,
            y: (1 - u) * quad[0].y + u * quad[1].y,
          };
          const bot = {
            x: (1 - u) * quad[3].x + u * quad[2].x,
            y: (1 - u) * quad[3].y + u * quad[2].y,
          };
          const imgP = {
            x: (1 - v) * top.x + v * bot.x,
            y: (1 - v) * top.y + v * bot.y,
          };
          return toScreen(imgP);
        };

        const pTL = bilinear(u0 + 0.2 * du, v0 + 0.2 * dv);
        const pTR = bilinear(u1 - 0.2 * du, v0 + 0.2 * dv);
        const pBR = bilinear(u1 - 0.2 * du, v1 - 0.2 * dv);
        const pBL = bilinear(u0 + 0.2 * du, v1 - 0.2 * dv);

        gridPolygons.push(`${pTL.x},${pTL.y} ${pTR.x},${pTR.y} ${pBR.x},${pBR.y} ${pBL.x},${pBL.y}`);
      }
    }
  }

  const screenCorners = quad.map(toScreen);

  return (
    <div className="w-full h-full flex flex-col min-h-0 bg-[var(--bb-space)] select-none">
      {/* Light-Table Top Metadata Status HUD */}
      <div className="h-8 shrink-0 px-3 bg-[var(--bb-vacuum)] border-b border-[var(--bb-border)] flex items-center justify-between text-[11px] font-mono text-[var(--bb-smoke)]">
        <div className="flex items-center gap-3">
          <span className="text-[var(--bb-gold)] font-bold">
            OPTICAL LIGHT-TABLE
          </span>
          <span className="text-[var(--bb-ash)]">|</span>
          <span>{imageWidth}×{imageHeight} PX</span>
          <span className="text-[var(--bb-ash)]">|</span>
          <span className="text-[var(--bb-sand)]">24-PATCH BILINEAR</span>
        </div>
        <div className="flex items-center gap-2">
          {quad.map((p, i) => (
            <span
              key={i}
              className="px-1.5 py-0.5 bg-[var(--bb-panel)] text-[10px] text-[var(--bb-sand)] border border-[var(--bb-border)]"
            >
              {cornerLabels[i]}: {Math.round(p.x)},{Math.round(p.y)}
            </span>
          ))}
        </div>
      </div>

      {/* Fully Contained Image Viewport */}
      <div
        ref={containerRef}
        className="relative flex-1 min-h-0 w-full bg-[var(--bb-vacuum)] overflow-hidden cursor-crosshair-custom"
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        {/* Letterboxed Canvas Area */}
        {imageSrc && box.w > 0 ? (
          <img
            src={imageSrc}
            alt="Reference preview"
            style={{
              position: 'absolute',
              left: `${box.x}px`,
              top: `${box.y}px`,
              width: `${box.w}px`,
              height: `${box.h}px`,
            }}
            className="pointer-events-none object-fill"
            draggable={false}
          />
        ) : !imageSrc ? (
          <button
            type="button"
            onClick={onBrowse}
            className="absolute inset-0 w-full h-full flex items-center justify-center bg-transparent border-none cursor-pointer group"
          >
            <div className="p-8 bg-[var(--bb-surface)] border border-[var(--bb-border-bright)] group-hover:border-[var(--bb-gold)] text-center space-y-2 transition-colors">
              <div className="text-xs font-bold tracking-widest text-[var(--bb-amber)] group-hover:text-[var(--bb-gold)]">
                + CLICK TO OPEN OR DROP REFERENCE FRAME HERE
              </div>
              <div className="text-[10px] text-[var(--bb-smoke)] tracking-wide">
                SUPPORTS DNG (RAW), JPEG, AND PNG COLORCHECKER CAPTURES
              </div>
            </div>
          </button>
        ) : null}

        {/* Precision Reticle & Sample Box SVG Layer */}
        {box.w > 0 && box.h > 0 && (
          <svg
            className="absolute inset-0 w-full h-full pointer-events-none"
            style={{ overflow: 'visible' }}
          >
            <defs>
              <g id="corner-reticle">
                <circle r="7" fill="rgba(61, 10, 5, 0.4)" stroke="#f28f1f" strokeWidth="1.5" />
                <line x1="-12" y1="0" x2="12" y2="0" stroke="#f5b931" strokeWidth="1" />
                <line x1="0" y1="-12" x2="0" y2="12" stroke="#f5b931" strokeWidth="1" />
                <circle r="1.8" fill="#fffdf2" />
              </g>
            </defs>

            {/* Boundary Quad Polygon */}
            <polygon
              points={screenCorners.map((p) => `${p.x},${p.y}`).join(' ')}
              fill="rgba(235, 100, 21, 0.05)"
              stroke="#eb6415"
              strokeWidth="1.5"
            />

            {/* 24-Patch Central Sampling Boxes */}
            {gridPolygons.map((pts, idx) => (
              <polygon
                key={`patch-box-${idx}`}
                points={pts}
                fill="rgba(242, 143, 31, 0.08)"
                stroke="#f28f1f"
                strokeWidth="0.8"
                strokeDasharray="2,2"
              />
            ))}

            {/* Draggable Corner Reticles */}
            {screenCorners.map((p, idx) => (
              <g
                key={`reticle-${idx}`}
                transform={`translate(${p.x}, ${p.y})`}
                className="pointer-events-auto cursor-move"
                onPointerDown={handlePointerDown(idx)}
              >
                {/* Invisible, generous hit area so the pointer cannot slip off mid-drag. */}
                <circle r="16" fill="transparent" />
                <use href="#corner-reticle" className="pointer-events-none" />
                <text
                  className="pointer-events-none"
                  x="12"
                  y="-8"
                  fill="#f5b931"
                  fontSize="10"
                  fontFamily="JetBrains Mono, monospace"
                  fontWeight="700"
                  stroke="#040201"
                  strokeWidth="3"
                  paintOrder="stroke"
                >
                  {cornerLabels[idx]}
                </text>
              </g>
            ))}
          </svg>
        )}
      </div>
    </div>
  );
};
