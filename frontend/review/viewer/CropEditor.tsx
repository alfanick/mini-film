/** Own a reversible crop draft in hooks until the photographer presses OK.
 * The original camera image and saved output stay separate layers so an existing crop can be expanded. */
import type { JSX } from "preact";
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { CropRect, Dimensions, Point, RetouchSettings, ReviewImage, ReviewProfileRender } from "../core/types";
import { CROP_RATIO_PRESETS } from "../core/constants";
import { versionedUrl } from "../core/selectors";
import {
  aspectLockedCrop,
  clamp,
  cropGestureMetrics,
  cropRectAround,
  defaultCrop,
  fitCropToRatio,
  formatCropRatio,
  fullFrameCrop,
  normalizeCropRect,
  normalizeRotation,
  ratiosMatch,
  rotatedSafeDimensions,
} from "./geometry";

export interface CropEditorProps {
  image: ReviewImage;
  selected: ReviewProfileRender | null;
  retouch: RetouchSettings;
  available: Dimensions;
  shortcutsBlocked: boolean;
  onReadyChange: (ready: boolean) => void;
  onApply: (retouch: RetouchSettings) => void;
  onCancel: () => void;
}

interface RatioChoice {
  key: string;
  rotated: boolean;
}
interface CropMemory {
  center: Point;
  area: number;
}
interface Drag {
  pointerId: number;
  handle: string;
  start: Point;
  crop: CropRect;
  rect: DOMRect;
}
interface TouchCrop {
  distance: number;
  angle: number;
  crop: CropRect;
  rotation: number;
  rect: DOMRect;
}
interface CropMedia {
  source: string | null;
  current: string | null;
}

/** Resolve named ratios in source-pixel coordinates; free crop deliberately returns null. */
function ratioFor(choice: RatioChoice, source: Dimensions | null): number | null {
  if (choice.key === "free") return null;
  let ratio: number;
  if (choice.key === "original") ratio = source ? source.width / source.height : 1;
  else if (choice.key === "a3-a4") ratio = Math.SQRT2;
  else {
    const [width, height] = choice.key.split(":").map(Number);
    ratio = width > 0 && height > 0 ? width / height : 1;
  }
  return choice.rotated ? 1 / ratio : ratio;
}

/** Remember crop area once so switching ratios does not progressively shrink the selection. */
function remember(crop: CropRect, source: Dimensions, rotation: number): CropMemory {
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  return {
    center: { x: crop.x + crop.width / 2, y: crop.y + crop.height / 2 },
    area: crop.width * safe.width * crop.height * safe.height,
  };
}

/** Reconstruct the remembered area for a new ratio, shrinking only to stay inside the safe frame. */
function resizeFromMemory(
  memory: CropMemory,
  crop: CropRect,
  ratio: number | null,
  source: Dimensions,
  rotation: number,
): CropRect {
  if (ratio === null) return normalizeCropRect(crop);
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  const width = Math.sqrt(memory.area * ratio);
  const height = Math.sqrt(memory.area / ratio);
  const scale = Math.min(1, safe.width / width, safe.height / height);
  return cropRectAround(memory.center, (width * scale) / safe.width, (height * scale) / safe.height);
}

/** Infer an existing named ratio while preserving custom crops as freely adjustable selections. */
function inferRatio(crop: CropRect, source: Dimensions, rotation: number): RatioChoice {
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  const actual = (crop.width * safe.width) / Math.max(1, crop.height * safe.height);
  for (const [key] of CROP_RATIO_PRESETS) {
    const base = ratioFor({ key, rotated: false }, source);
    if (base === null) continue;
    if (ratiosMatch(actual, base)) return { key, rotated: false };
    if (ratiosMatch(actual, 1 / base)) return { key, rotated: true };
  }
  return { key: "free", rotated: false };
}

/** Render crop controls and handle captured pointer gestures without mutating rendered DOM. */
export function CropEditor({
  image,
  selected,
  retouch,
  available,
  shortcutsBlocked,
  onReadyChange,
  onApply,
  onCancel,
}: CropEditorProps): JSX.Element {
  const [saved] = useState<RetouchSettings>(() => structuredClone(retouch));
  const [media] = useState<CropMedia>(() => {
    const url = image.crop_source_url || selected?.base_url || image.preview_url || selected?.url;
    const updated = image.crop_source_url
      ? image.crop_source_updated_at || image.preview_updated_at
      : selected?.base_url
        ? selected.updated_at
        : image.preview_url
          ? image.preview_updated_at
          : selected?.updated_at;
    return {
      source: url ? versionedUrl(url, updated) : null,
      current: selected?.url ? versionedUrl(selected.url, selected.updated_at) : null,
    };
  });
  const [crop, setCrop] = useState<CropRect>(retouch.crop || defaultCrop());
  const [rotation, setRotation] = useState<number>(retouch.rotation_degrees);
  const [sourceSize, setSourceSize] = useState<Dimensions | null>(null);
  const [choice, setChoice] = useState<RatioChoice>({ key: "original", rotated: false });
  const [memory, setMemory] = useState<CropMemory | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const pointers = useRef<Map<number, Point>>(new Map());
  const drag = useRef<Drag | null>(null);
  const touch = useRef<TouchCrop | null>(null);
  const safe = sourceSize ? rotatedSafeDimensions(sourceSize.width, sourceSize.height, rotation) : null;
  const scale = safe ? Math.min(available.width / safe.width, available.height / safe.height) : 1;
  const targetRatio = ratioFor(choice, sourceSize);
  useEffect((): (() => void) => {
    onReadyChange(Boolean(sourceSize));
    return (): void => onReadyChange(false);
  }, [sourceSize, onReadyChange]);

  /** Use camera dimensions while honoring the orientation of the extracted preview. */
  function loaded(event: JSX.TargetedEvent<HTMLImageElement>): void {
    if (sourceSize) return;
    const natural = { width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight };
    if (natural.width < 1 || natural.height < 1) return;
    let width = Number(image.source_width);
    let height = Number(image.source_height);
    if (!Number.isFinite(width) || width < 1 || !Number.isFinite(height) || height < 1) {
      width = natural.width;
      height = natural.height;
    } else if (width > height !== natural.width > natural.height) [width, height] = [height, width];
    const dimensions = { width, height };
    const initialChoice = saved.crop ? inferRatio(crop, dimensions, rotation) : choice;
    const initialCrop = saved.crop
      ? crop
      : fitCropToRatio(crop, ratioFor(initialChoice, dimensions), dimensions, rotation);
    setSourceSize(dimensions);
    setChoice(initialChoice);
    setCrop(initialCrop);
    setMemory(remember(initialCrop, dimensions, rotation));
  }

  /** Commit the draft as one retouch operation; all pointer updates remain local until this point. */
  function approve(): void {
    onApply({ ...retouch, crop, rotation_degrees: rotation });
  }

  /** Change a named aspect ratio using the same remembered area and center. */
  const chooseRatio = useCallback(
    (next: RatioChoice): void => {
      if (!sourceSize) return;
      const remembered = memory || remember(crop, sourceSize, rotation);
      setMemory(remembered);
      setChoice(next);
      setCrop(resizeFromMemory(remembered, crop, ratioFor(next, sourceSize), sourceSize, rotation));
    },
    [sourceSize, memory, crop, rotation],
  );

  /** Rotate the ratio independently from image rotation, matching the R shortcut. */
  const rotateRatio = useCallback((): void => {
    if (targetRatio === null || ratiosMatch(targetRatio, 1)) return;
    chooseRatio({ ...choice, rotated: !choice.rotated });
  }, [targetRatio, choice, chooseRatio]);

  /** Keep the crop area stable when the safe frame changes during image rotation. */
  function rotate(value: number): void {
    const next = normalizeRotation(value);
    if (sourceSize) {
      const remembered = memory || remember(crop, sourceSize, rotation);
      setMemory(remembered);
      setCrop(resizeFromMemory(remembered, crop, targetRatio, sourceSize, next));
    }
    setRotation(next);
  }

  useEffect((): (() => void) => {
    /** Rotate the ratio even while its select/range owns focus, matching the original plain-R shortcut. */
    function keydown(event: KeyboardEvent): void {
      const target = event.target;
      if (
        event.defaultPrevented ||
        shortcutsBlocked ||
        event.ctrlKey ||
        event.metaKey ||
        event.altKey ||
        (target instanceof Element && target.matches("#tags, #notes, #min-rating"))
      )
        return;
      if (event.key.toLowerCase() === "r") {
        event.preventDefault();
        event.stopImmediatePropagation();
        rotateRatio();
      }
    }
    window.addEventListener("keydown", keydown, true);
    return (): void => window.removeEventListener("keydown", keydown, true);
  }, [rotateRatio, shortcutsBlocked]);

  /** Begin dragging a handle or create a two-pointer pinch/rotation gesture. */
  function pointerDown(event: JSX.TargetedPointerEvent<HTMLDivElement>): void {
    if (!sourceSize || !overlayRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      /* Browser may cancel a touch before capture. */
    }
    const rect = overlayRef.current.getBoundingClientRect();
    if (pointers.current.size >= 2) {
      const metrics = cropGestureMetrics(Array.from(pointers.current.values()).slice(0, 2), rect);
      drag.current = null;
      touch.current = { distance: Math.max(1, metrics.distance), angle: metrics.angle, crop, rotation, rect };
    } else {
      drag.current = {
        pointerId: event.pointerId,
        handle: event.target instanceof HTMLElement ? event.target.dataset.cropHandle || "move" : "move",
        start: { x: event.clientX, y: event.clientY },
        crop,
        rect,
      };
    }
  }

  /** Update the draft geometry from captured pointer deltas, without saving intermediate frames. */
  function pointerMove(event: JSX.TargetedPointerEvent<HTMLDivElement>): void {
    if (!pointers.current.has(event.pointerId) || !sourceSize) return;
    event.preventDefault();
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const gesture = touch.current;
    const currentDrag = drag.current;
    let next: CropRect;
    let nextRotation = rotation;
    if (gesture && pointers.current.size >= 2) {
      const metrics = cropGestureMetrics(Array.from(pointers.current.values()).slice(0, 2), gesture.rect);
      const ratio = metrics.distance / gesture.distance;
      nextRotation = normalizeRotation(gesture.rotation + ((metrics.angle - gesture.angle) * 180) / Math.PI);
      next = fitCropToRatio(
        cropRectAround(metrics.center, gesture.crop.width * ratio, gesture.crop.height * ratio),
        targetRatio,
        sourceSize,
        nextRotation,
      );
      setRotation(nextRotation);
    } else if (currentDrag?.pointerId === event.pointerId) {
      const dx = (event.clientX - currentDrag.start.x) / Math.max(1, currentDrag.rect.width);
      const dy = (event.clientY - currentDrag.start.y) / Math.max(1, currentDrag.rect.height);
      next =
        currentDrag.handle === "move"
          ? normalizeCropRect({ ...currentDrag.crop, x: currentDrag.crop.x + dx, y: currentDrag.crop.y + dy })
          : aspectLockedCrop(currentDrag.crop, currentDrag.handle, dx, dy, sourceSize, rotation, targetRatio);
    } else return;
    setCrop(next);
    setMemory(remember(next, sourceSize, nextRotation));
  }

  /** Release pointer bookkeeping when a browser ends or cancels its captured gesture. */
  function pointerEnd(event: JSX.TargetedPointerEvent<HTMLDivElement>): void {
    pointers.current.delete(event.pointerId);
    if (pointers.current.size < 2) touch.current = null;
    if (drag.current?.pointerId === event.pointerId) drag.current = null;
  }

  let savedStyle: JSX.CSSProperties = {};
  if (sourceSize && safe) {
    // Transform the already-rendered crop back over the uncropped source so its boundaries remain editable.
    const oldSafe = rotatedSafeDimensions(sourceSize.width, sourceSize.height, saved.rotation_degrees);
    const oldCrop = saved.crop || fullFrameCrop();
    const delta = normalizeRotation(rotation - saved.rotation_degrees);
    const radians = (delta * Math.PI) / 180;
    const x = (oldCrop.x + oldCrop.width / 2) * oldSafe.width - oldSafe.width / 2;
    const y = (oldCrop.y + oldCrop.height / 2) * oldSafe.height - oldSafe.height / 2;
    const width = oldCrop.width * oldSafe.width * scale;
    const height = oldCrop.height * oldSafe.height * scale;
    savedStyle = {
      left: (safe.width / 2 + Math.cos(radians) * x - Math.sin(radians) * y) * scale - width / 2,
      top: (safe.height / 2 + Math.sin(radians) * x + Math.cos(radians) * y) * scale - height / 2,
      width,
      height,
      transform: `rotate(${delta}deg)`,
    };
  }
  return (
    <>
      <div
        id="crop-stage"
        class="crop-stage"
        hidden={!sourceSize}
        style={{ width: safe ? safe.width * scale : 1, height: safe ? safe.height * scale : 1 }}
      >
        <div id="crop-canvas" class="crop-canvas">
          <img
            id="crop-source-image"
            class="crop-source-image"
            src={media.source || undefined}
            alt=""
            draggable={false}
            decoding="async"
            onLoad={loaded}
            style={{
              width: sourceSize ? sourceSize.width * scale : undefined,
              height: sourceSize ? sourceSize.height * scale : undefined,
              transform: `translate(-50%, -50%) rotate(${rotation}deg)`,
            }}
          />
          <img
            id="crop-current-image"
            class="crop-current-image"
            src={media.current || undefined}
            alt=""
            draggable={false}
            decoding="async"
            hidden={!media.current || !sourceSize}
            style={savedStyle}
          />
          <div id="crop-overlay" ref={overlayRef} class="crop-overlay" hidden={!sourceSize}>
            <div
              id="crop-box"
              class="crop-box"
              style={{
                left: `${crop.x * 100}%`,
                top: `${crop.y * 100}%`,
                width: `${crop.width * 100}%`,
                height: `${crop.height * 100}%`,
              }}
              onPointerDown={pointerDown}
              onPointerMove={pointerMove}
              onPointerUp={pointerEnd}
              onPointerCancel={pointerEnd}
            >
              {(["nw", "ne", "sw", "se"] as const).map((handle) => (
                <span key={handle} data-crop-handle={handle} />
              ))}
            </div>
          </div>
        </div>
      </div>
      <div id="crop-tools" class="crop-tools">
        <button id="crop-rotate-left" type="button" onClick={(): void => rotate(rotation - 90)}>
          -90
        </button>
        <label class="crop-rotation-control">
          <span>Rotate</span>
          <input
            id="crop-rotation"
            type="range"
            min={-180}
            max={180}
            step={0.25}
            value={clamp(rotation, -180, 180)}
            onInput={(event): void => rotate(Number(event.currentTarget.value))}
          />
          <output id="crop-rotation-value">{`${rotation > 0 ? "+" : ""}${rotation.toFixed(1)}°`}</output>
        </label>
        <button id="crop-rotate-right" type="button" onClick={(): void => rotate(rotation + 90)}>
          +90
        </button>
        <label class="crop-ratio-control">
          <span>Ratio</span>
          <select
            id="crop-ratio"
            disabled={!sourceSize}
            value={choice.key}
            onChange={(event): void => chooseRatio({ key: event.currentTarget.value, rotated: false })}
          >
            <option value="current" hidden>
              Current
            </option>
            {CROP_RATIO_PRESETS.map(([value, label, rotatedLabel]) => (
              <option key={value} value={value}>
                {value === choice.key && value === "original" && targetRatio
                  ? `Original ${formatCropRatio(targetRatio)}`
                  : value === choice.key && choice.rotated && value !== "1:1"
                    ? rotatedLabel || value.split(":").reverse().join(":")
                    : label}
              </option>
            ))}
          </select>
        </label>
        <div id="crop-actions" class="crop-actions" aria-label="Crop actions">
          <button
            id="crop-reset"
            type="button"
            onClick={(): void => onApply({ ...retouch, crop: null, rotation_degrees: 0 })}
          >
            Clear
          </button>
          <button id="crop-cancel" type="button" onClick={onCancel}>
            Cancel
          </button>
          <button id="crop-ok" class="crop-apply" type="button" disabled={!sourceSize} onClick={approve}>
            OK
          </button>
        </div>
      </div>
    </>
  );
}
