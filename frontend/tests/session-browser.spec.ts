/**
 * Exercise manual metadata ownership and navigation against both implementations.
 * Delayed responses make lost drafts and incorrect image targeting observable.
 */
import { required } from "./required";
import { expect, test } from "@playwright/test";
import type { ReviewUpdateRequest } from "../review/core/types";
import { openReview, sendState } from "./harness";

// Autosaving must not normalize focused text, discard duplicate tags, or disturb the caret.
test("focused tags retain raw text and caret while unrelated metadata follows SSE", async ({ page }) => {
  const harness = await openReview(page);
  const tags = page.locator("#tags");
  const raw = " 12,12 007 ";
  await tags.fill(raw);
  await tags.evaluate((input: HTMLInputElement): void => input.setSelectionRange(3, 3));
  await expect.poll(() => required(harness.data.images[0]).tags).toEqual(["12", "12", "007"]);
  await expect(tags).toHaveValue(raw);
  await expect(tags).toBeFocused();
  const snapshot = structuredClone(harness.data);
  required(snapshot.images[0]).notes = "Incoming camera note";
  await sendState(page, snapshot);
  await expect(page.locator("#notes")).toHaveValue("Incoming camera note");
  await expect(tags).toHaveValue(raw);
  expect(await tags.evaluate((input: HTMLInputElement): number | null => input.selectionStart)).toBe(3);
  await tags.press("Tab");
  await expect(tags).toHaveValue("12, 12, 007");
  expect(harness.errors).toEqual([]);
});

// The queued second edit must remain visible while the server acknowledges the first one.
test("a later note survives an older autosave acknowledgement and stale SSE", async ({ page }) => {
  const harness = await openReview(page);
  const saves: ReviewUpdateRequest[] = [];
  let releaseFirst: (() => void) | undefined;
  const firstReady = new Promise<void>((resolve): void => {
    releaseFirst = resolve;
  });
  await page.route("**/api/review", async (route): Promise<void> => {
    const body = route.request().postDataJSON() as ReviewUpdateRequest;
    saves.push(body);
    if (saves.length === 1) await firstReady;
    required(harness.data.images[0]).notes = body.notes;
    await route.fulfill({ json: { ...harness.data, type: "patch" } });
  });
  const notes = page.locator("#notes");
  await notes.fill("First draft");
  await expect.poll(() => saves.length).toBe(1);
  await notes.fill("Second draft");
  await sendState(page, structuredClone(harness.data));
  await expect(notes).toHaveValue("Second draft");
  releaseFirst?.();
  await expect.poll(() => saves.length).toBeGreaterThanOrEqual(2);
  await expect.poll(() => required(harness.data.images[0]).notes).toBe("Second draft");
  await expect(notes).toHaveValue("Second draft");
  expect(saves.slice(0, 2).map((body): string => body.notes)).toEqual(["First draft", "Second draft"]);
  expect(saves.every((body): boolean => body.image_id === 1)).toBe(true);
  expect(harness.errors).toEqual([]);
});

// The next picture may publish a different look from the previously selected profile.
test("navigation carries the next published profile without modifying its metadata", async ({ page }) => {
  const harness = await openReview(page);
  required(harness.data.images[1]).publish_profile_indexes = [1];
  required(harness.data.images[1]).selected_profile_index = 0;
  required(harness.data.images[1]).notes = "Second camera note";
  await sendState(page, structuredClone(harness.data));
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  await expect(page.locator("#profile-state")).toContainText("Soft");
  await expect.poll(() => required(harness.data.images[1]).selected_profile_index).toBe(1);
  await expect(page.locator("#notes")).toHaveValue("Second camera note");
  const saves = harness.requests.filter((request): boolean => request.path === "review");
  expect(saves.map((request): number => (request.body as ReviewUpdateRequest).image_id)).toEqual([1, 2]);
  expect(required(saves[1]).body).toMatchObject({
    image_id: 2,
    selected_profile_index: 1,
    publish_profile_indexes: [1],
    notes: "Second camera note",
    advance_after_update: false,
  });
  expect(harness.errors).toEqual([]);
});
