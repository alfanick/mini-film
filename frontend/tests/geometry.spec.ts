/** Verify crop, autofocus, and zoom geometry independently of browser timing.
 * These bounds protect photo content when the review UI changes its rendering mechanism. */
import { expect, test } from "@playwright/test";
import type { CropRect } from "../review/core/types";
import { createState } from "../review/core/state";
import {
  cropRectAround,
  fullFrameCrop,
  normalizeCropRect,
  ratiosMatch,
  rotatedSafeDimensions,
  focusRegionPolygons,
  fullZoomOffset,
  zoomLoupePosition,
} from "../review/viewer/geometry";
import { reviewFixture } from "./fixtures";
import { histogramBins } from "../review/viewer/Histogram";
import { nearbyPreloadUrls } from "../review/viewer/Preloads";

/** Check physical frame bounds rather than duplicating the crop implementation. */
function expectInsideFrame(crop: CropRect): void {
  expect(crop.x).toBeGreaterThanOrEqual(0);
  expect(crop.y).toBeGreaterThanOrEqual(0);
  expect(crop.width).toBeGreaterThanOrEqual(0.01);
  expect(crop.height).toBeGreaterThanOrEqual(0.01);
  expect(crop.x + crop.width).toBeLessThanOrEqual(1);
  expect(crop.y + crop.height).toBeLessThanOrEqual(1);
}

test("rotation keeps the safe output rectangle inside the source photograph", (): void => {
  for (const [width, height] of [
    [6000, 4000],
    [4000, 6000],
    [4000, 4000],
  ]) {
    for (const rotation of [-180, -135, -90, -45, -15, 0, 15, 45, 90, 135, 180]) {
      const safe = rotatedSafeDimensions(width, height, rotation);
      const radians = (rotation * Math.PI) / 180;
      const cos = Math.abs(Math.cos(radians));
      const sin = Math.abs(Math.sin(radians));
      expect(safe.width).toBeGreaterThan(0);
      expect(safe.height).toBeGreaterThan(0);
      expect(safe.width * cos + safe.height * sin).toBeLessThanOrEqual(width + 1e-8);
      expect(safe.width * sin + safe.height * cos).toBeLessThanOrEqual(height + 1e-8);
    }
  }
  expect(rotatedSafeDimensions(6000, 4000, 90)).toEqual({ width: 4000, height: 6000 });
  expect(rotatedSafeDimensions(6000, 4000, 180)).toEqual({ width: 6000, height: 4000 });
});

test("crop normalization constrains missing, nonfinite, and out-of-frame rectangles", (): void => {
  expect(normalizeCropRect(null)).toEqual(fullFrameCrop());
  expect(normalizeCropRect({ width: Number.NaN, height: Infinity })).toEqual(fullFrameCrop());
  for (const x of [-2, 0, 0.5, 2]) {
    for (const width of [-1, 0, 0.3, 1, 2]) {
      expectInsideFrame(normalizeCropRect({ x, y: x, width, height: width }));
    }
  }
});

test("resizing a crop keeps its center until a frame edge constrains it", (): void => {
  const crop = cropRectAround({ x: 0.25, y: 0.75 }, 0.2, 0.4);
  expect(crop.x + crop.width / 2).toBeCloseTo(0.25);
  expect(crop.y + crop.height / 2).toBeCloseTo(0.75);
  expectInsideFrame(cropRectAround({ x: -1, y: 2 }, 0.2, 0.4));
  expectInsideFrame(cropRectAround({ x: 0.5, y: 0.5 }, 3, 4));
});

test("free crop ratios never compare as a locked ratio", (): void => {
  expect(ratiosMatch(null, null)).toBe(false);
  expect(ratiosMatch(null, 1)).toBe(false);
  expect(ratiosMatch(Number.NaN, 1)).toBe(false);
  expect(ratiosMatch(1.5, 3 / 2)).toBe(true);
  expect(ratiosMatch(1.5, 4 / 3)).toBe(false);
});

test("full zoom cannot pan past an image edge and centers smaller images", (): void => {
  expect(fullZoomOffset(0, 0, 100, 200)).toBe(50);
  for (const pointer of [0, 100, 200]) {
    for (const relative of [0, 0.5, 1]) {
      const offset = fullZoomOffset(pointer, relative, 500, 200);
      expect(offset).toBeGreaterThanOrEqual(-300);
      expect(offset).toBeLessThanOrEqual(0);
    }
  }
});

test("mouse and touch loupes remain inside the viewer at every corner", (): void => {
  const viewer = { left: 100, top: 200, width: 900, height: 600 };
  for (const pointerType of ["mouse", "touch"]) {
    for (const clientX of [viewer.left, viewer.left + viewer.width]) {
      for (const clientY of [viewer.top, viewer.top + viewer.height]) {
        const loupe = zoomLoupePosition(clientX, clientY, viewer, 180, 180, pointerType);
        expect(loupe.left).toBeGreaterThanOrEqual(0);
        expect(loupe.top).toBeGreaterThanOrEqual(0);
        expect(loupe.left + 180).toBeLessThanOrEqual(viewer.width);
        expect(loupe.top + 180).toBeLessThanOrEqual(viewer.height);
      }
    }
  }
});

test("camera focus regions follow crop and rotation and disappear outside the crop", (): void => {
  const image = reviewFixture().images[0];
  const original = focusRegionPolygons(image, image.retouch)[0];
  image.retouch.rotation_degrees = 180;
  const rotated = focusRegionPolygons(image, image.retouch)[0];
  expect(rotated.primary).toBe(original.primary);
  for (const [index, point] of rotated.points.entries()) {
    expect(point.x).toBeCloseTo(1 - original.points[index].x);
    expect(point.y).toBeCloseTo(1 - original.points[index].y);
  }
  image.retouch.rotation_degrees = 0;
  image.retouch.crop = { x: 0.25, y: 0.25, width: 0.5, height: 0.5 };
  const cropped = focusRegionPolygons(image, image.retouch)[0];
  for (const [index, point] of cropped.points.entries()) {
    expect(point.x).toBeCloseTo((original.points[index].x - 0.25) / 0.5);
    expect(point.y).toBeCloseTo((original.points[index].y - 0.25) / 0.5);
  }
  image.retouch.crop = { x: 0.75, y: 0, width: 0.25, height: 1 };
  expect(focusRegionPolygons(image, image.retouch)).toEqual([]);
});

test("histograms ignore transparent samples while counting the exact visible RGB and luma bins", (): void => {
  const bins = histogramBins(new Uint8ClampedArray([255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0]));
  expect(bins.red[255]).toBe(1);
  expect(bins.green[255]).toBe(1);
  expect(bins.blue[255]).toBe(0);
  expect(bins.luma[54]).toBe(1);
  expect(bins.luma[182]).toBe(1);
  expect(bins.luma.reduce((sum, count) => sum + count, 0)).toBe(2);
});

test("RAW browsing preloads both neighbors and respects optimistic profile selection", (): void => {
  const state = createState();
  state.data = reviewFixture();
  state.pendingProfileSelections.set(3, 1);
  const urls = nearbyPreloadUrls(state, 2, 2560);
  expect(urls).toHaveLength(2);
  expect(urls[0]).toContain(state.data.images[0].profiles[0].url);
  expect(urls[1]).toContain(state.data.images[2].profiles[1].url);
});

test("compressed-only browsing preloads forward full media only on large viewports", (): void => {
  const state = createState();
  state.data = reviewFixture();
  state.data.images = state.data.images.map((image) => ({
    ...image,
    source_type: "compressed",
    processing_mode: "direct",
    full_url: `/media/${image.id}/full`,
  }));
  expect(nearbyPreloadUrls(state, 1, 2560).map((url) => url.split("?")[0])).toEqual(["/media/2/full", "/media/3/full"]);
  expect(nearbyPreloadUrls(state, 1, 2048)[0]).toContain(state.data.images[1].preview_url);
  expect(nearbyPreloadUrls(state, 3, 2560)).toEqual([]);
});
