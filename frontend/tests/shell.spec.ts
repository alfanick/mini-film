/** Compare subtle shell and editing contracts against both the legacy and compiled review applications.
 * These cases protect focus ownership and portable slider deltas that broad screenshots cannot establish. */
import { expect, test } from "@playwright/test";
import type { ReviewUpdateRequest } from "../review/core/types";
import { openReview, sendState } from "./harness";

test("crop tools advertise saved adjustments and do not open without usable source media", async ({ page }) => {
  const harness = await openReview(page);
  const data = structuredClone(harness.data);
  data.images[0].retouch.rotation_degrees = 15;
  await sendState(page, data);
  await expect(page.locator("#crop-toggle")).toHaveClass(/active/);
  await expect(page.locator("#crop-toggle")).toHaveAttribute("title", "Crop or rotation adjustment active");
  data.images[0].retouch.rotation_degrees = 0;
  data.images[0].crop_source_url = null;
  data.images[0].preview_url = null;
  data.images[0].profiles = data.images[0].profiles.map((profile) => ({ ...profile, url: null, base_url: null }));
  await sendState(page, data);
  await expect(page.locator("#crop-toggle")).toHaveAttribute("title", "Crop/rotate");
  await page.locator("#crop-toggle").click();
  await expect(page.locator("#crop-tools")).toBeHidden();
  expect(harness.errors).toEqual([]);
});

test("global information shortcut remains active from the color filter but not the rating filter", async ({ page }) => {
  const harness = await openReview(page);
  await page.locator("#filter-label").focus();
  await page.keyboard.press("i");
  await expect(page.locator("#focus-overlay")).toBeVisible();
  await page.locator("#min-rating").focus();
  await page.keyboard.press("i");
  await expect(page.locator("#focus-overlay")).toBeVisible();
  expect(harness.errors).toEqual([]);
});

test("focused retouch inputs survive autosave acknowledgements and later server changes", async ({ page }) => {
  const harness = await openReview(page);
  const exposure = page.locator("#retouch-exposure");
  await exposure.focus();
  await page.keyboard.press("ArrowRight");
  await expect(exposure).toHaveValue("0.8");
  await expect.poll(() => harness.requests.filter((request) => request.path === "review").length).toBeGreaterThan(0);
  const data = structuredClone(harness.data);
  data.images[0].retouch.adjustments.exposure = 2;
  await sendState(page, data);
  await expect(exposure).toBeFocused();
  await expect(exposure).toHaveValue("0.8");
  const beforeCommit = harness.requests.filter((request) => request.path === "review").length;
  await page.keyboard.press("Enter");
  await expect
    .poll(() => harness.requests.filter((request) => request.path === "review").length)
    .toBeGreaterThan(beforeCommit);
  const requests = harness.requests.filter((request) => request.path === "review");
  const committed = requests[requests.length - 1].body as ReviewUpdateRequest;
  expect(committed.retouch?.adjustments.exposure).toBe(0.8);
  expect(harness.errors).toEqual([]);
});

test("editing one slider normalizes every displayed delta clipped by the selected profile baseline", async ({
  page,
}) => {
  const harness = await openReview(page);
  const data = structuredClone(harness.data);
  data.profiles[0].retouch_base.exposure = 3;
  data.images[0].retouch.adjustments.exposure = 3;
  await sendState(page, data);
  await expect(page.locator("#retouch-exposure")).toHaveValue("4");
  await page.locator("#retouch-contrast").focus();
  await page.keyboard.press("ArrowRight");
  await expect
    .poll(() => {
      const requests = harness.requests.filter((item) => item.path === "review");
      const request = requests[requests.length - 1];
      return (request?.body as ReviewUpdateRequest | undefined)?.retouch?.adjustments.exposure;
    })
    .toBe(1);
  expect(harness.errors).toEqual([]);
});
