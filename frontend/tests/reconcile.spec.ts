/**
 * Lock down the legacy snapshot/patch and selector rules independently of browser rendering.
 * These regressions protect multi-client selection and user-owned metadata during live updates.
 */
import { required } from "./required";
import { expect, test } from "@playwright/test";
import { reviewFixture } from "./fixtures";
import { createState } from "../review/core/state";
import { mergeSnapshot, reconcileReview } from "../review/core/reconcile";
import {
  defaultRetouch,
  filteredImages,
  normalizedRetouch,
  profileDisplayState,
  selectedProfile,
} from "../review/core/selectors";
import type { ReviewState } from "../review/core/types";

/** Build an isolated established review session from the same fixture used by browser tests. */
function session(): ReviewState {
  const initial = createState();
  return { ...initial, ...reconcileReview(initial, reviewFixture()) };
}

test("incremental updates preserve unmentioned metadata and explicit image order", (): void => {
  const state = session();
  const previous = state.data;
  if (!previous) throw new Error("Fixture requires a snapshot");
  const patched = mergeSnapshot(previous, {
    type: "patch",
    version: previous.version,
    images: [{ ...required(previous.images[0]), rating: 5 }],
    removed_image_ids: [2],
    image_ids: [3, 1],
  });
  expect(patched?.images.map((image) => image.id)).toEqual([3, 1]);
  expect(required(patched?.images[1]).notes).toBe(required(previous.images[0]).notes);
  expect(required(patched?.images[1]).rating).toBe(5);
  expect(required(previous.images[0]).rating).not.toBe(5);
  expect(previous.images).toHaveLength(3);
});

test("patches without an initial snapshot wait for a complete state", (): void => {
  expect(mergeSnapshot(null, { type: "patch", version: "22.15.1", client_count: 3 })).toBeNull();
});

test("optimistic selection survives stale SSE until the exact selection is acknowledged", (): void => {
  const state = session();
  const data = reviewFixture();
  const pending = required(required(data.images[0]).profiles[1]).profile_index;
  state.pendingProfileSelections.set(1, pending);
  const merged = { ...state, ...reconcileReview(state, data) };
  expect(selectedProfile(required(merged.data?.images[0]) || null, merged)?.profile_index).toBe(pending);
  expect(merged.pendingProfileSelections.get(1)).toBe(pending);
  required(data.images[0]).selected_profile_index = pending;
  const acknowledged = reconcileReview(merged, data);
  expect(acknowledged.pendingProfileSelections?.has(1)).toBe(false);
});

test("older snapshots cannot reverse a confirmed newer profile selection", (): void => {
  const state = session();
  const data = reviewFixture();
  if (!state.data) throw new Error("Fixture requires a snapshot");
  const chosen = required(required(state.data.images[0]).profiles[1]).profile_index;
  state.data.images[0] = {
    ...required(state.data.images[0]),
    selected_profile_index: chosen,
    updated_at: "2026-09-06",
  };
  required(data.images[0]).updated_at = "2026-09-05";
  expect(required(reconcileReview(state, data).data?.images[0]).selected_profile_index).toBe(chosen);
});

test("server removals choose the first remaining visible picture", (): void => {
  const state = session();
  const merged = { ...state, ...reconcileReview(state, { type: "patch", version: "22.15.1", removed_image_ids: [1] }) };
  expect(merged.currentId).toBe(2);
  expect(filteredImages(merged).map((image) => image.id)).toEqual([2, 3]);
});

test("neutral defaults and partial edits do not share mutable crop or adjustment objects", (): void => {
  const first = defaultRetouch(),
    second = defaultRetouch();
  first.adjustments.exposure = 2;
  expect(second.adjustments.exposure).toBe(0);
  expect(
    normalizedRetouch({
      adjustments: { exposure: 8, temperature: -3000 },
      crop: { x: 0.9, y: 0.9, width: 0.4, height: 0.5 },
      rotation_degrees: 450,
    }),
  ).toEqual({
    adjustments: { ...second.adjustments, exposure: 4, temperature: -2500 },
    crop: { x: 0.6, y: 0.5, width: 0.4, height: 0.5 },
    rotation_degrees: 90,
  });
});

test("profile rendering statuses retain their original user-facing wording", (): void => {
  const image = required(reviewFixture().images[0]);
  const profile = required(image.profiles[0]);
  expect(profileDisplayState(image, null, true)).toEqual({
    state: "waiting",
    text: "waiting",
    title: "waiting for profile render",
  });
  expect(profileDisplayState(image, { ...profile, status: "queued", retouch_pending: true }).text).toBe(
    "retouch queued",
  );
  expect(profileDisplayState(image, { ...profile, status: "processing", retouch_pending: true }).text).toBe(
    "retouch rendering",
  );
  expect(profileDisplayState(image, profile, true).text).toBe("retouch draft");
  expect(
    profileDisplayState(
      { ...image, source_type: "compressed", processing_mode: "direct", preview_status: "done" },
      null,
    ).text,
  ).toBe("ready");
});
