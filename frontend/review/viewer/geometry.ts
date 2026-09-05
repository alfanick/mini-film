/** Pure photo geometry shared by hook-driven crop, focus, and zoom views.
 * Keeping these calculations independent of browser state preserves render parity and makes their bounds testable. */
import type * as T from "../core/types";
import { ZOOM_LOUPE_POINTER_GAP_PX, ZOOM_LOUPE_TOUCH_GAP_PX } from "../core/constants";

export interface CropGestureMetrics {
  center: T.Point;
  distance: number;
  angle: number;
}
export interface LoupePosition {
  left: number;
  top: number;
}
export interface FocusPolygon {
  points: T.Point[];
  primary: boolean;
}

/** Constrain an adjustment or coordinate to its supported interval. */
export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/** Keep signed rotations in the same half-turn interval used by RAW rendering. */
export function normalizeRotation(value: number): number {
  let rotation = Number.isFinite(value) ? value % 360 : 0;
  if (rotation > 180) rotation -= 360;
  if (rotation < -180) rotation += 360;
  return Math.abs(rotation) < 0.0001 ? 0 : rotation;
}

/** Start new crops with an inset frame that leaves room for dragging. */
export function defaultCrop(): T.CropRect {
  return { x: 0.1, y: 0.1, width: 0.8, height: 0.8 };
}

/** Represent the complete source in normalized image coordinates. */
export function fullFrameCrop(): T.CropRect {
  return { x: 0, y: 0, width: 1, height: 1 };
}

/** Recognize exact portrait and landscape orientation swaps. */
export function isQuarterTurn(rotation: number): boolean {
  return Math.abs(Math.abs(normalizeRotation(rotation)) - 90) < 0.001;
}

/** Recognize rotations that preserve the original safe dimensions. */
export function isHalfTurn(rotation: number): boolean {
  return Math.abs(Math.abs(normalizeRotation(rotation)) - 180) < 0.001;
}

/** Find the largest centered rectangle that avoids black corners after rotation. */
export function rotatedSafeDimensions(width: number, height: number, rotation: number): T.Dimensions {
  const normalized = normalizeRotation(rotation);
  if (Math.abs(normalized) < 0.001 || isHalfTurn(normalized)) return { width, height };
  if (isQuarterTurn(normalized)) return { width: height, height: width };

  const radians = (Math.abs(normalized) * Math.PI) / 180;
  const sin = Math.abs(Math.sin(radians));
  const cos = Math.abs(Math.cos(radians));
  const longSide = Math.max(width, height);
  const shortSide = Math.min(width, height);
  let safeWidth;
  let safeHeight;
  if (shortSide <= 2 * sin * cos * longSide || Math.abs(sin - cos) < Number.EPSILON) {
    const side = 0.5 * shortSide;
    if (width >= height) {
      safeWidth = side / sin;
      safeHeight = side / cos;
    } else {
      safeWidth = side / cos;
      safeHeight = side / sin;
    }
  } else {
    const cos2 = cos * cos - sin * sin;
    safeWidth = (width * cos - height * sin) / cos2;
    safeHeight = (height * cos - width * sin) / cos2;
  }
  return {
    width: Math.max(1, Math.min(width, Math.floor(safeWidth))),
    height: Math.max(1, Math.min(height, Math.floor(safeHeight))),
  };
}

/** Normalize incomplete or out-of-frame crops without losing the minimum handle area. */
export function normalizeCropRect(crop: Partial<T.CropRect> | null | undefined): T.CropRect {
  const requestedWidth = Number(crop?.width);
  const requestedHeight = Number(crop?.height);
  const requestedX = Number(crop?.x);
  const requestedY = Number(crop?.y);
  const width = clamp(Number.isFinite(requestedWidth) ? requestedWidth : 1, 0.01, 1);
  const height = clamp(Number.isFinite(requestedHeight) ? requestedHeight : 1, 0.01, 1);
  return {
    x: clamp(Number.isFinite(requestedX) ? requestedX : 0, 0, 1 - width),
    y: clamp(Number.isFinite(requestedY) ? requestedY : 0, 0, 1 - height),
    width,
    height,
  };
}

/** Resize around a remembered center and constrain the result to the source frame. */
export function cropRectAround(center: T.Point, width: number, height: number): T.CropRect {
  const normalizedWidth = clamp(width, 0.01, 1);
  const normalizedHeight = clamp(height, 0.01, 1);
  return normalizeCropRect({
    x: clamp(center.x, 0, 1) - normalizedWidth / 2,
    y: clamp(center.y, 0, 1) - normalizedHeight / 2,
    width: normalizedWidth,
    height: normalizedHeight,
  });
}

/** Compare named aspect ratios with the existing rounding tolerance; null means free crop. */
export function ratiosMatch(left: number | null, right: number | null): boolean {
  if (left === null || right === null || !Number.isFinite(left) || !Number.isFinite(right)) return false;
  return Math.abs(Math.log(Math.max(0.0001, left) / Math.max(0.0001, right))) < 0.004;
}

/** Express a measured aspect ratio as a small readable integer fraction. */
export function formatCropRatio(ratio: number): string {
  let bestNumerator = ratio;
  let bestDenominator = 1;
  let bestError = Infinity;
  for (let denominator = 1; denominator <= 20; denominator += 1) {
    const numerator = Math.max(1, Math.round(ratio * denominator));
    const error = Math.abs(ratio - numerator / denominator);
    if (error < bestError) {
      bestNumerator = numerator;
      bestDenominator = denominator;
      bestError = error;
    }
  }
  return `${bestNumerator}:${bestDenominator}`;
}

/** Fit a crop to its requested pixel aspect ratio in the rotated safe source. */
export function fitCropToRatio(
  crop: T.CropRect,
  ratio: number | null,
  source: T.Dimensions | null,
  rotation: number,
): T.CropRect {
  if (!source || ratio === null || !Number.isFinite(ratio) || ratio <= 0) return normalizeCropRect(crop);
  const rect = normalizeCropRect(crop);
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  let pixelWidth = rect.width * safe.width;
  let pixelHeight = rect.height * safe.height;
  if (pixelWidth / pixelHeight > ratio) {
    pixelWidth = pixelHeight * ratio;
  } else {
    pixelHeight = pixelWidth / ratio;
  }
  return cropRectAround(
    { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 },
    pixelWidth / safe.width,
    pixelHeight / safe.height,
  );
}

/** Resize from a fixed opposite corner, preserving either free edges or the locked ratio. */
export function aspectLockedCrop(
  start: T.CropRect,
  handle: string,
  dx: number,
  dy: number,
  source: T.Dimensions | null,
  rotation: number,
  targetRatio: number | null,
): T.CropRect {
  if (!source) return normalizeCropRect(start);
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  const anchorX = handle.includes("w") ? start.x + start.width : start.x;
  const anchorY = handle.includes("n") ? start.y + start.height : start.y;
  const signX = handle.includes("w") ? -1 : 1;
  const signY = handle.includes("n") ? -1 : 1;
  const targetWidth = signX > 0 ? start.width + dx : start.width - dx;
  const targetHeight = signY > 0 ? start.height + dy : start.height - dy;
  if (targetRatio === null) {
    const maxWidth = signX > 0 ? 1 - anchorX : anchorX;
    const maxHeight = signY > 0 ? 1 - anchorY : anchorY;
    const width = clamp(Math.abs(targetWidth), Math.min(0.01, maxWidth), maxWidth);
    const height = clamp(Math.abs(targetHeight), Math.min(0.01, maxHeight), maxHeight);
    return normalizeCropRect({
      x: signX > 0 ? anchorX : anchorX - width,
      y: signY > 0 ? anchorY : anchorY - height,
      width,
      height,
    });
  }
  const normalizedRatio = (targetRatio * safe.height) / safe.width;
  let width = Math.min(Math.abs(targetWidth), Math.abs(targetHeight) * normalizedRatio);
  const maxWidthX = signX > 0 ? 1 - anchorX : anchorX;
  const maxHeight = signY > 0 ? 1 - anchorY : anchorY;
  const maxWidth = Math.min(maxWidthX, maxHeight * normalizedRatio);
  const minWidth = Math.min(maxWidth, Math.max(0.01, normalizedRatio * 0.01));
  width = clamp(width, minWidth, maxWidth);
  const height = width / normalizedRatio;
  return normalizeCropRect({
    x: signX > 0 ? anchorX : anchorX - width,
    y: signY > 0 ? anchorY : anchorY - height,
    width,
    height,
  });
}

/** Measure the center, scale, and angle of a two-pointer crop gesture. */
export function cropGestureMetrics(points: T.Point[], rect: DOMRect): CropGestureMetrics {
  const [first, second] = points;
  return {
    center: {
      x: ((first.x + second.x) / 2 - rect.left) / Math.max(1, rect.width),
      y: ((first.y + second.y) / 2 - rect.top) / Math.max(1, rect.height),
    },
    distance: Math.hypot(second.x - first.x, second.y - first.y),
    angle: Math.atan2(second.y - first.y, second.x - first.x),
  };
}

/** Keep the enlarged image under the pointer without panning beyond its edges. */
export function fullZoomOffset(pointer: number, relative: number, contentSize: number, frameSize: number): number {
  if (contentSize <= frameSize) return (frameSize - contentSize) / 2;
  return clamp(pointer - relative * contentSize, frameSize - contentSize, 0);
}

/** Place mouse and touch loupes near the pointer while keeping them inside the viewer. */
export function zoomLoupePosition(
  clientX: number,
  clientY: number,
  viewerRect: Pick<DOMRect, "left" | "top" | "width" | "height">,
  loupeWidth: number,
  loupeHeight: number,
  pointerType: string,
): LoupePosition {
  const pointerX = clientX - viewerRect.left;
  const pointerY = clientY - viewerRect.top;
  const gap = pointerType === "touch" ? ZOOM_LOUPE_TOUCH_GAP_PX : ZOOM_LOUPE_POINTER_GAP_PX;
  const maxLeft = Math.max(0, viewerRect.width - loupeWidth);
  const maxTop = Math.max(0, viewerRect.height - loupeHeight);
  const rightFits = pointerX + gap + loupeWidth <= viewerRect.width;
  const aboveFits = pointerY - gap - loupeHeight >= 0;
  const preferRight = rightFits || pointerX < viewerRect.width / 2;
  const preferAbove = aboveFits || pointerY >= viewerRect.height / 2;
  const left = preferRight ? pointerX + gap : pointerX - loupeWidth - gap;
  const top = preferAbove ? pointerY - loupeHeight - gap : pointerY + gap;

  return {
    left: clamp(left, 0, maxLeft),
    top: clamp(top, 0, maxTop),
  };
}

/** Escape a media URL for a CSS background-image value. */
export function cssUrl(value: string | null): string {
  return String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

/** Project camera autofocus regions through rotation and crop into the displayed frame. */
export function focusRegionPolygons(image: T.ReviewImage, retouch: T.RetouchSettings): FocusPolygon[] {
  const frameWidth = Number(image?.exif?.focus_frame_width);
  const frameHeight = Number(image?.exif?.focus_frame_height);
  const regions = image?.exif?.focus_regions || [];
  if (
    !Number.isFinite(frameWidth) ||
    frameWidth <= 0 ||
    !Number.isFinite(frameHeight) ||
    frameHeight <= 0 ||
    regions.length === 0
  ) {
    return [];
  }

  const rotation = normalizeRotation(retouch.rotation_degrees);
  const safe = rotatedSafeDimensions(frameWidth, frameHeight, rotation);
  const crop = retouch.crop || fullFrameCrop();
  const cropLeft = crop.x * safe.width;
  const cropTop = crop.y * safe.height;
  const cropWidth = crop.width * safe.width;
  const cropHeight = crop.height * safe.height;
  const radians = (rotation * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);

  return regions.flatMap((region) => {
    const left = Number(region.x) * frameWidth;
    const top = Number(region.y) * frameHeight;
    const right = (Number(region.x) + Number(region.width)) * frameWidth;
    const bottom = (Number(region.y) + Number(region.height)) * frameHeight;
    if (![left, top, right, bottom].every(Number.isFinite) || right <= left || bottom <= top) return [];
    const points = [
      { x: left, y: top },
      { x: right, y: top },
      { x: right, y: bottom },
      { x: left, y: bottom },
    ].map((point) => {
      const x = point.x - frameWidth / 2;
      const y = point.y - frameHeight / 2;
      return {
        x: (cos * x - sin * y + safe.width / 2 - cropLeft) / cropWidth,
        y: (sin * x + cos * y + safe.height / 2 - cropTop) / cropHeight,
      };
    });
    const xValues = points.map((point) => point.x);
    const yValues = points.map((point) => point.y);
    if (
      Math.max(...xValues) <= 0 ||
      Math.min(...xValues) >= 1 ||
      Math.max(...yValues) <= 0 ||
      Math.min(...yValues) >= 1
    ) {
      return [];
    }
    return [{ points, primary: Boolean(region.primary) }];
  });
}
