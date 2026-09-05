/** Property tests cover broad crop and incremental-state input spaces with deterministic, reproducible seeds. */
import { expect, test } from "@playwright/test";
import fc from "fast-check";
import { normalizeCropRect, cropRectAround, normalizeRotation } from "../review/viewer/geometry";
import { mergeSnapshot } from "../review/core/reconcile";
import { reviewFixture } from "./fixtures";

test("crop normalization is bounded and idempotent for arbitrary finite coordinates", (): void => {
  fc.assert(
    fc.property(
      fc.record({
        x: fc.double({ noNaN: true, noDefaultInfinity: true }),
        y: fc.double({ noNaN: true, noDefaultInfinity: true }),
        width: fc.double({ noNaN: true, noDefaultInfinity: true }),
        height: fc.double({ noNaN: true, noDefaultInfinity: true }),
      }),
      (input): void => {
        const crop = normalizeCropRect(input);
        expect(crop.x).toBeGreaterThanOrEqual(0);
        expect(crop.y).toBeGreaterThanOrEqual(0);
        expect(crop.width).toBeGreaterThanOrEqual(0.01);
        expect(crop.height).toBeGreaterThanOrEqual(0.01);
        expect(crop.x + crop.width).toBeLessThanOrEqual(1);
        expect(crop.y + crop.height).toBeLessThanOrEqual(1);
        expect(normalizeCropRect(crop)).toEqual(crop);
        expect(
          cropRectAround({ x: crop.x + crop.width / 2, y: crop.y + crop.height / 2 }, crop.width, crop.height).width,
        ).toBe(crop.width);
      },
    ),
    { seed: 230001, numRuns: 500 },
  );
});

test("rotation normalization stays bounded and idempotent", (): void => {
  fc.assert(
    fc.property(fc.double({ noNaN: true, noDefaultInfinity: true }), (rotation): void => {
      const value = normalizeRotation(rotation);
      expect(value).toBeGreaterThanOrEqual(-180);
      expect(value).toBeLessThanOrEqual(180);
      expect(normalizeRotation(value)).toBe(value);
    }),
    { seed: 230002, numRuns: 500 },
  );
});

test("unrelated patches preserve every image and new diffusion settings are never dropped", (): void => {
  fc.assert(
    fc.property(fc.integer({ min: 0, max: 100 }), fc.nat({ max: 1000 }), (softness, clientCount): void => {
      const previous = reviewFixture();
      const original = structuredClone(previous);
      const settings = { ...previous.diffusion_default, softness };
      const next = mergeSnapshot(previous, {
        type: "patch",
        version: previous.version,
        client_count: clientCount,
        diffusion_default: settings,
        profile_diffusion_settings: [{ profile_index: 0, settings }],
      });
      expect(next?.images).toBe(previous.images);
      expect(next?.diffusion_default.softness).toBe(softness);
      expect(next?.profile_diffusion_settings).toEqual([{ profile_index: 0, settings }]);
      expect(previous).toEqual(original);
    }),
    { seed: 230003, numRuns: 200 },
  );
});
