import React, { useRef, useState } from 'react';
import type { Point, ChartQuad } from '../types';

interface Props {
  imageSrc?: string;
  imageWidth?: number;
  imageHeight?: number;
  quad: ChartQuad;
  onQuadChange: (quad: ChartQuad) => void;
  disabled?: boolean;
}

export const LightTableOverlay: React.FC<Props> = ({
  imageSrc,
  imageWidth = 480,
  imageHeight = 320,
  quad,
  onQuadChange,
  disabled = false,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [activeCorner, setActiveCorner] = useState<number | null>(null);


  const cornerLabels = ['TL [0,0]', 'TR [5,0]', 'BR [5,3]', 'BL [0,3]'];

  // Handle SVG coordinate transformation on drag
  const handlePointerDown = (index: number) => (e: React.PointerEvent) => {
    if (disabled) return;
    (e.target as Element).setPointerCapture(e.pointerId);
    setActiveCorner(index);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    if (activeCorner === null || disabled || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    
    // Convert client coords to normalized image coordinate space (imageWidth x imageHeight)
    const relX = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));

    const newX = relX * imageWidth;
    const newY = relY * imageHeight;

    const newQuad = [...quad] as ChartQuad;
    newQuad[activeCorner] = { x: Math.round(newX * 10) / 10, y: Math.round(newY * 10) / 10 };
    onQuadChange(newQuad);
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    if (activeCorner !== null) {
      try {
        (e.target as Element).releasePointerCapture(e.pointerId);
      } catch {}
      setActiveCorner(null);
    }
  };

  // Generate 24 patch sample center grid boxes
  const gridCells = [];
  for (let r = 0; r < 4; r++) {
    for (let c = 0; c < 6; c++) {
      const u0 = c / 6;
      const u1 = (c + 1) / 6;
      const v0 = r / 4;
      const v1 = (r + 1) / 4;

      // Sample central 60% box (u in [u0 + 0.2du, u1 - 0.2du])
      const du = u1 - u0;
      const dv = v1 - v0;
      const su0 = u0 + 0.2 * du;
      const su1 = u1 - 0.2 * du;
      const sv0 = v0 + 0.2 * dv;
      const sv1 = v1 - 0.2 * dv;

      const bilinear = (u: number, v: number): Point => {
        const top = {
          x: (1 - u) * quad[0].x + u * quad[1].x,
          y: (1 - u) * quad[0].y + u * quad[1].y,
        };
        const bot = {
          x: (1 - u) * quad[3].x + u * quad[2].x,
          y: (1 - u) * quad[3].y + u * quad[2].y,
        };
        return {
          x: (1 - v) * top.x + v * bot.x,
          y: (1 - v) * top.y + v * bot.y,
        };
      };

      const pTL = bilinear(su0, sv0);
      const pTR = bilinear(su1, sv0);
      const pBR = bilinear(su1, sv1);
      const pBL = bilinear(su0, sv1);

      gridCells.push(
        <polygon
          key={`cell-${r}-${c}`}
          points={`${pTL.x},${pTL.y} ${pTR.x},${pTR.y} ${pBR.x},${pBR.y} ${pBL.x},${pBL.y}`}
          fill="rgba(245, 109, 24, 0.08)"
          stroke="#f56d18"
          strokeWidth="0.8"
          strokeDasharray="2,2"
        />
      );
    }
  }

  return (
    <div className="relative w-full h-full flex flex-col items-center justify-center p-4">
      {/* Light-Table Top Precision Status Header */}
      <div className="w-full flex items-center justify-between pb-2 mb-2 border-b border-[var(--bb-border)] text-xs text-[var(--bb-smoke)]">
        <div className="flex items-center gap-3">
          <span className="text-[var(--bb-gold)] font-semibold flex items-center gap-1">
            <span className="inline-block w-2 h-2 rounded-full bg-[var(--bb-amber)] animate-ping"></span>
            OPTICAL LIGHT-TABLE
          </span>
          <span>DIM: {imageWidth}×{imageHeight}px</span>
          <span>CHART: 24-PATCH CLASSIC</span>
        </div>
        <div className="flex items-center gap-4">
          <span className="text-[var(--bb-ash)]">CROSSHAIR PINS: 4-PT BILINEAR WARP</span>
          <div className="flex gap-1">
            {quad.map((p, i) => (
              <span key={i} className="px-1.5 py-0.5 bg-[var(--bb-panel)] text-[var(--bb-sand)] text-[10px] border border-[var(--bb-border)]">
                P{i}: {Math.round(p.x)},{Math.round(p.y)}
              </span>
            ))}
          </div>
        </div>
      </div>

      {/* Main Viewport Container */}
      <div
        ref={containerRef}
        className="relative w-full aspect-[3/2] max-h-[580px] bg-[var(--bb-vacuum)] light-table-viewport border border-[var(--bb-border)] rounded-sm overflow-hidden flex items-center justify-center scanlines cursor-crosshair-custom"
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        {/* Background Image / Placeholder Pattern */}
        {imageSrc ? (
          <img
            src={imageSrc}
            alt="Reference preview"
            className="w-full h-full object-contain pointer-events-none"
            draggable={false}
          />
        ) : (
          <div className="w-full h-full light-table-grid flex items-center justify-center">
            <div className="text-center space-y-2 p-6 bg-[var(--bb-space)]/90 border border-[var(--bb-border-bright)]">
              <p className="text-[var(--bb-amber)] font-semibold tracking-wider text-sm">NO REFERENCE IMAGE LOADED</p>
              <p className="text-[var(--bb-smoke)] text-xs">SELECT OR DROP A RAW (DNG) OR STINKY JPEG FRAME ONTO THE LIGHT-TABLE</p>
            </div>
          </div>
        )}

        {/* Precision Coordinate & Grid Overlay */}
        <svg
          viewBox={`0 0 ${imageWidth} ${imageHeight}`}
          className="absolute inset-0 w-full h-full pointer-events-auto"
          preserveAspectRatio="xMidYMid meet"
        >
          <defs>
            {/* Corner Marker Reticle */}
            <g id="reticle-pin">
              <circle r="8" fill="rgba(196, 39, 12, 0.2)" stroke="#fa9723" strokeWidth="1.5" />
              <line x1="-14" y1="0" x2="14" y2="0" stroke="#fcc238" strokeWidth="1" />
              <line x1="0" y1="-14" x2="0" y2="14" stroke="#fcc238" strokeWidth="1" />
              <circle r="2" fill="#fffdf2" />
            </g>
          </defs>

          {/* Chart Boundary Polygon */}
          <polygon
            points={`${quad[0].x},${quad[0].y} ${quad[1].x},${quad[1].y} ${quad[2].x},${quad[2].y} ${quad[3].x},${quad[3].y}`}
            fill="rgba(245, 109, 24, 0.06)"
            stroke="#f56d18"
            strokeWidth="1.8"
          />

          {/* Bilinear 24-Patch Sample Boxes */}
          {gridCells}

          {/* Draggable Corner Pins */}
          {quad.map((p, idx) => (
            <g
              key={`corner-${idx}`}
              transform={`translate(${p.x}, ${p.y})`}
              className="cursor-move hover:scale-125 transition-transform"
              onPointerDown={handlePointerDown(idx)}
            >
              <use href="#reticle-pin" />
              <text
                x="14"
                y="-10"
                fill="#fcc238"
                fontSize="11"
                fontFamily="JetBrains Mono, monospace"
                fontWeight="700"
                filter="drop-shadow(0 0 2px #000)"
              >
                {cornerLabels[idx]}
              </text>
            </g>
          ))}
        </svg>

        {/* Optical Loupe Coordinates HUD (Bottom Left) */}
        <div className="absolute bottom-3 left-3 bg-[var(--bb-space)]/90 border border-[var(--bb-border)] px-3 py-1.5 text-[10px] text-[var(--bb-sand)] font-mono flex items-center gap-3">
          <span className="text-[var(--bb-gold)]">RETICLE WARP LOCK</span>
          <span>CALIBRATED MATRIX REGION 6×4</span>
          <span className="text-[var(--bb-smoke)]">[CLICK & DRAG PIN MARKERS TO ALIGN]</span>
        </div>
      </div>
    </div>
  );
};
