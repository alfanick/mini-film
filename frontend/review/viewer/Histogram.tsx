/** Draw the review histogram from the displayed image in a canvas effect.
 * Canvas is an explicit rendering boundary; visibility, empty states, and cleanup remain owned by Preact. */
import type { JSX, RefObject } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { clamp } from "./geometry";

interface HistogramProps {
  imageRef: RefObject<HTMLImageElement>;
  sourceKey: string;
  filter: string;
  visible: boolean;
  revision: number;
}

export interface HistogramBins {
  luma: Uint32Array;
  red: Uint32Array;
  green: Uint32Array;
  blue: Uint32Array;
}

/** Count visible pixels into matching luma and RGB bins, ignoring fully transparent samples. */
export function histogramBins(pixels: Uint8ClampedArray): HistogramBins {
  const bins = {
    luma: new Uint32Array(256),
    red: new Uint32Array(256),
    green: new Uint32Array(256),
    blue: new Uint32Array(256),
  };
  for (let index = 0; index < pixels.length; index += 4) {
    if (pixels[index + 3] === 0) continue;
    const red = pixels[index];
    const green = pixels[index + 1];
    const blue = pixels[index + 2];
    if (red === undefined || green === undefined || blue === undefined) continue;
    const luma = clamp(Math.round(red * 0.2126 + green * 0.7152 + blue * 0.0722), 0, 255);
    bins.red[red] = (bins.red[red] ?? 0) + 1;
    bins.green[green] = (bins.green[green] ?? 0) + 1;
    bins.blue[blue] = (bins.blue[blue] ?? 0) + 1;
    bins.luma[luma] = (bins.luma[luma] ?? 0) + 1;
  }
  return bins;
}

/** Render the four normalized channels with the original review grid and colors. */
function drawHistogram(canvas: HTMLCanvasElement, bins: HistogramBins): boolean {
  const rect = canvas.getBoundingClientRect();
  const pixelRatio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round((rect.width || 512) * pixelRatio));
  const height = Math.max(1, Math.round((rect.height || 128) * pixelRatio));
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return false;
  ctx.strokeStyle = "rgba(255, 255, 255, 0.12)";
  ctx.lineWidth = Math.max(1, width / 512);
  ctx.beginPath();
  for (const x of [0.25, 0.5, 0.75]) {
    ctx.moveTo(Math.round(width * x), 0);
    ctx.lineTo(Math.round(width * x), height);
  }
  ctx.moveTo(0, Math.round(height * 0.5));
  ctx.lineTo(width, Math.round(height * 0.5));
  ctx.stroke();
  const channels: [Uint32Array, string, boolean][] = [
    [bins.luma, "rgba(255, 255, 255, 0.84)", true],
    [bins.red, "rgba(255, 74, 74, 0.92)", false],
    [bins.green, "rgba(65, 210, 116, 0.92)", false],
    [bins.blue, "rgba(85, 154, 255, 0.92)", false],
  ];
  for (const [channel, color, fill] of channels) {
    const max = Math.max(...channel);
    if (max <= 0) continue;
    ctx.strokeStyle = color;
    ctx.lineWidth = fill ? Math.max(1, width / 512) : Math.max(1.2, width / 380);
    ctx.beginPath();
    if (fill) ctx.moveTo(0, height);
    for (const [index, count] of channel.entries()) {
      const x = (index / 255) * width;
      const y = height - (count / max) * (height - 2);
      if (index === 0 && !fill) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    if (fill) {
      ctx.lineTo(width, height);
      ctx.closePath();
      ctx.fillStyle = "rgba(255, 255, 255, 0.3)";
      ctx.fill();
    }
    ctx.stroke();
  }
  return true;
}

/** Sample only a small preview and redraw when media, draft adjustments, or viewer dimensions change. */
export function Histogram({ imageRef, sourceKey, filter, visible, revision }: HistogramProps): JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const sampleRef = useRef<HTMLCanvasElement>(null);
  const [message, setMessage] = useState<string>("No image");
  useEffect((): (() => void) | undefined => {
    if (!visible) return;
    const timer = window.setTimeout((): void => {
      const image = imageRef.current;
      const canvas = canvasRef.current;
      const sample = sampleRef.current;
      // An empty/loading result must not leave the previous photograph's graph visible underneath its message.
      const chartContext = canvas?.getContext("2d");
      if (chartContext && canvas) chartContext.clearRect(0, 0, canvas.width, canvas.height);
      if (!sourceKey || !image || !canvas || !sample) {
        setMessage("No image");
        return;
      }
      if (!image.complete || image.naturalWidth < 1 || image.naturalHeight < 1) {
        setMessage("Loading");
        return;
      }
      const scale = Math.min(1, 512 / Math.max(image.naturalWidth, image.naturalHeight));
      sample.width = Math.max(1, Math.round(image.naturalWidth * scale));
      sample.height = Math.max(1, Math.round(image.naturalHeight * scale));
      const ctx = sample.getContext("2d", { willReadFrequently: true });
      if (!ctx) {
        setMessage("Unavailable");
        return;
      }
      try {
        ctx.filter = filter || "none";
        ctx.drawImage(image, 0, 0, sample.width, sample.height);
        const drawn = drawHistogram(canvas, histogramBins(ctx.getImageData(0, 0, sample.width, sample.height).data));
        setMessage(drawn ? "" : "Unavailable");
      } catch (error: unknown) {
        console.error(error);
        setMessage("Unavailable");
      }
    }, 100);
    return (): void => window.clearTimeout(timer);
  }, [imageRef, sourceKey, filter, visible, revision]);
  return (
    <div id="histogram-overlay" class="histogram-overlay" hidden={!visible} aria-label="Histogram">
      <canvas id="histogram-canvas" ref={canvasRef} width={512} height={128} />
      <canvas ref={sampleRef} hidden style={{ display: "none" }} />
      <div id="histogram-empty" class="histogram-empty" hidden={!message}>
        {message}
      </div>
    </div>
  );
}
