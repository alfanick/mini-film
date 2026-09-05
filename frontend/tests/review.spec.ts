// Browser behavior checks run unchanged against the old and compiled review UIs.
// Optional local screenshot parity catches layout changes during the migration.
import { expect, test } from "@playwright/test";
import type { Page, TestInfo } from "@playwright/test";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import type { DiffusionSettings, ReviewUpdateRequest } from "../review/core/types";
import { diffusionFixture, samplerFixture } from "./fixtures";

import { openReview, sendState } from "./harness";

/** Whole-image differences retain separate counts for one-level rounding and larger antialiasing variance. */
interface ScreenshotDifference {
  changedPixels: number;
  maximumChannelDifference: number;
  pixelsAboveOneLevel: number;
}

/** Decode screenshots in the browser to distinguish one-level compositor rounding from actual layout changes. */
async function screenshotDifference(page: Page, expected: Buffer, actual: Buffer): Promise<ScreenshotDifference> {
  return page.evaluate(
    async ([expectedBase64, actualBase64]) => {
      /** Decode PNG pixels without adding a platform-specific image dependency to CI. */
      const pixels = async (encoded: string): Promise<ImageData> => {
        const image = new Image();
        image.src = `data:image/png;base64,${encoded}`;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.naturalWidth;
        canvas.height = image.naturalHeight;
        const context = canvas.getContext("2d");
        if (!context) throw new Error("Screenshot comparison requires a canvas context");
        context.drawImage(image, 0, 0);
        return context.getImageData(0, 0, canvas.width, canvas.height);
      };
      const [before, after] = await Promise.all([pixels(expectedBase64), pixels(actualBase64)]);
      if (before.width !== after.width || before.height !== after.height)
        return {
          changedPixels: Number.MAX_SAFE_INTEGER,
          maximumChannelDifference: 255,
          pixelsAboveOneLevel: Number.MAX_SAFE_INTEGER,
        };
      let changedPixels = 0;
      let maximumChannelDifference = 0;
      let pixelsAboveOneLevel = 0;
      for (let index = 0; index < before.data.length; index += 4) {
        let changed = false;
        let aboveOneLevel = false;
        for (let channel = 0; channel < 4; channel += 1) {
          const difference = Math.abs(before.data[index + channel] - after.data[index + channel]);
          changed ||= difference !== 0;
          aboveOneLevel ||= difference > 1;
          maximumChannelDifference = Math.max(maximumChannelDifference, difference);
        }
        if (changed) changedPixels += 1;
        if (aboveOneLevel) pixelsAboveOneLevel += 1;
      }
      return { changedPixels, maximumChannelDifference, pixelsAboveOneLevel };
    },
    [expected.toString("base64"), actual.toString("base64")],
  );
}

/** Wait for two identical compositor frames before comparing the fixed CSS-pixel viewport. */
async function settledScreenshot(page: Page): Promise<Buffer> {
  // Clear stale hover before comparing synchronous legacy mounts with reactive dialog commits.
  await page.mouse.move(0, 0);
  // Repaint rounded native clip layers from the same viewport history before capturing either implementation.
  const viewport = page.viewportSize();
  if (viewport) {
    await page.setViewportSize({ ...viewport, width: viewport.width + 1 });
    await page.setViewportSize(viewport);
  }
  // Font and layout callbacks must settle equally in both implementations before pixel comparison.
  await page.evaluate(async (): Promise<void> => {
    await document.fonts.ready;
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  // Captures here have no focused text caret; preserving caret styles avoids repainting native control layers.
  let previous = await page.screenshot({ animations: "disabled", scale: "css", caret: "initial" });
  const differences: ScreenshotDifference[] = [];
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const current = await page.screenshot({ animations: "disabled", scale: "css", caret: "initial" });
    if (current.equals(previous)) return current;
    differences.push(await screenshotDifference(page, previous, current));
    previous = current;
  }
  throw new Error(`Review screenshot did not settle to two identical frames: ${JSON.stringify(differences)}`);
}

/** Compare images only when explicitly requested, avoiding stale local baselines. */
async function compareBaseline(page: Page, info: TestInfo, name: string): Promise<void> {
  const directory = resolve("target/review-parity", info.project.name);
  await mkdir(directory, { recursive: true });
  const baselinePath = resolve(directory, `${name}-legacy.png`);
  const actual = await settledScreenshot(page);
  const legacy = process.env.REVIEW_LEGACY === "1";
  if (legacy && process.env.REVIEW_COMPARE_LEGACY !== "1") {
    await writeFile(baselinePath, actual);
  } else {
    await writeFile(resolve(directory, `${name}-${legacy ? "legacy-repeat" : "typescript"}.png`), actual);
    if (process.env.REVIEW_COMPARE_LEGACY === "1") {
      expect(existsSync(baselinePath), `${name} legacy screenshot exists`).toBe(true);
      const baseline = await readFile(baselinePath);
      if (!actual.equals(baseline)) {
        const difference = await screenshotDifference(page, baseline, actual);
        await info.attach(`${name}-pixel-difference`, {
          body: JSON.stringify(difference),
          contentType: "application/json",
        });
        // Repeated original Chromium publish captures varied at 43 rounded-corner pixels, with 17 above one level
        // and a maximum delta of 6/255. Other native controls added at most 29 one-level pixels. Keep that measured
        // exception confined to this scene, never mask regions, and reject every larger text or layout change.
        const nativePublishCorners = name === "publish-draft" && info.project.name === "chromium";
        expect(difference.maximumChannelDifference, `${name} channel difference`).toBeLessThanOrEqual(
          nativePublishCorners ? 6 : 1,
        );
        expect(difference.pixelsAboveOneLevel, `${name} pixels above one channel level`).toBeLessThanOrEqual(
          nativePublishCorners ? 17 : 0,
        );
        expect(difference.changedPixels, `${name} changed pixels`).toBeLessThanOrEqual(nativePublishCorners ? 80 : 64);
      }
    }
  }
}

test("initial state mounts once, keeps relative URLs, and renders desktop and mobile", async ({ page }, info) => {
  const harness = await openReview(page);
  await expect(page.locator(".app")).toHaveCount(1);
  await expect(page.locator("html")).toHaveAttribute("data-event-sources", "1");
  await expect(page.locator("#tags")).toHaveValue("12");
  await expect(page.locator("#notes")).toHaveValue("Camera note");
  await compareBaseline(page, info, "desktop-dark");
  await page.emulateMedia({ colorScheme: "light" });
  await compareBaseline(page, info, "desktop-light");
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('[data-mobile-drawer="metadata"]')).toBeVisible();
  await compareBaseline(page, info, "mobile-light");
  await page.locator('[data-mobile-drawer="metadata"]').click();
  await expect(page.locator("#notes")).toBeVisible();
  await page.keyboard.press("Escape");
  expect(harness.errors).toEqual([]);
  expect(harness.requests.filter((request) => request.path === "state")).toHaveLength(1);
  if (process.env.REVIEW_LEGACY !== "1") {
    expect(harness.scripts).toHaveLength(1);
    expect(harness.scripts[0]).toContain("/nested/review/assets/app.js");
  }
});

test("snapshots and SSE patches update ordering, selection, and removals", async ({ page }) => {
  const harness = await openReview(page);
  const first = structuredClone(harness.data.images[0]);
  first.file_name = "updated.NEF";
  await sendState(page, {
    type: "patch",
    version: harness.data.version,
    images: [first],
    image_ids: [3, 1],
    removed_image_ids: [2],
  });
  await expect(page.locator("#image-title")).toHaveText("updated.NEF");
  await expect(page.locator("#image-list")).not.toContainText("frame-2.NEF");
  const snapshot = structuredClone(harness.data);
  snapshot.ui.current_image_id = 3;
  await sendState(page, snapshot);
  await expect(page.locator("#image-title")).toHaveText("frame-3.NEF");
  await expect(page.locator("html")).toHaveAttribute("data-event-sources", "1");
  expect(harness.errors).toEqual([]);
});

test("metadata saves and rating/navigation shortcuts preserve save ordering", async ({ page }) => {
  const harness = await openReview(page);
  await page.locator("#notes").fill("Manual note");
  await page.locator("#notes").press("Tab");
  await expect.poll(() => harness.data.images[0].notes).toBe("Manual note");
  await page.evaluate(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  });
  await page.keyboard.press("5");
  await expect(page.locator("#image-title")).toHaveText("frame-2.NEF");
  expect(harness.data.images[0].rating).toBe(5);
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#image-title")).toHaveText("frame-3.NEF");
  await expect.poll(() => harness.data.ui.current_image_id).toBe(3);
  const saves = harness.requests.filter((request) => request.path === "review");
  expect((saves[0].body as ReviewUpdateRequest).notes).toBe("Manual note");
  expect(saves.filter((request) => (request.body as ReviewUpdateRequest).advance_after_update)).toHaveLength(1);
  expect(harness.errors).toEqual([]);
});

test("focus SVG visibility, histogram, zoom, and crop approve/cancel work", async ({ page }) => {
  const harness = await openReview(page);
  await expect(page.locator("#focus-overlay")).toHaveAttribute("hidden");
  await page.keyboard.press("i");
  await expect(page.locator("#focus-overlay")).not.toHaveAttribute("hidden");
  await expect(page.locator("#focus-overlay polygon")).toHaveCount(1);
  await page.keyboard.press("i");
  await expect(page.locator("#focus-overlay")).toHaveAttribute("hidden");
  await page.keyboard.press("h");
  await expect(page.locator("#histogram-overlay")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.locator("#main-image").dblclick();
  await expect(page.locator("#zoom-full")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.locator("#crop-toggle").click();
  await expect(page.locator("#crop-ratio")).toBeEnabled();
  await page.locator("#crop-ratio").selectOption("1:1");
  await page.locator("#crop-cancel").click();
  expect(harness.data.images[0].retouch.crop).toBeNull();
  await page.locator("#crop-toggle").click();
  await expect(page.locator("#crop-ratio")).toBeEnabled();
  await page.locator("#crop-ratio").selectOption("1:1");
  await page.locator("#crop-ok").click();
  await expect.poll(() => harness.data.images[0].retouch.crop).not.toBeNull();
  const crop = harness.data.images[0].retouch.crop;
  expect(crop).not.toBeNull();
  expect((crop!.width * 1200) / (crop!.height * 800)).toBeCloseTo(1, 4);
  expect(harness.errors).toEqual([]);
});

test("publish submits selected output and metadata settings", async ({ page }) => {
  const harness = await openReview(page);
  await page.locator("#publish").click();
  await page.locator("#publish-album").fill("Album test");
  await page.locator("#publish-size-mode").selectOption("long-edge");
  await page.locator("#publish-long-edge").fill("2048");
  await page.locator("#publish-jpg-quality").fill("85");
  await page.locator("#publish-strip-metadata").check();
  await page.locator("#publish-submit").click();
  await expect.poll(() => harness.requests.filter((request) => request.path === "publish").length).toBe(1);
  expect(harness.requests.find((request) => request.path === "publish")?.body).toMatchObject({
    album: "Album test",
    long_edge: 2048,
    jpg_quality: 85,
    strip_metadata: true,
    output_format: "jpg",
  });
  await expect(page.locator("#publish-status")).toContainText("Album test");
  expect(harness.errors).toEqual([]);
});

test("sampler, diffusion empty responses, and panorama dialogs remain usable", async ({ page }) => {
  const harness = await openReview(page);
  await page.locator("#sampler").click();
  await expect(page.locator("#sampler-overlay")).toContainText("Complete");
  await page.keyboard.press("Escape");
  await expect(page.locator("#sampler-overlay")).toBeHidden();
  await page.locator("#diffusion").click();
  await expect(page.locator("#diffusion-overlay")).toBeVisible();
  await page.getByRole("button", { name: "Reset current", exact: true }).click();
  await expect(page.locator("#diffusion-overlay")).toBeHidden();
  expect(harness.requests.find((request) => request.path === "diffusion/settings")).toMatchObject({
    method: "DELETE",
    body: { image_id: 1, profile_index: 0, scope: "current" },
  });
  await page.locator("#panorama").click();
  await expect(page.locator("#panorama-overlay")).toBeVisible();
  await expect(page.locator(".panorama-source")).toHaveCount(3);
  await page.keyboard.press("Escape");
  await expect(page.locator("#panorama-overlay")).toBeHidden();
  expect(harness.errors).toEqual([]);
});

test("pending profile selection survives an older SSE response", async ({ page }) => {
  const harness = await openReview(page);
  let completeSave: (() => void) | undefined;
  const saveReady = new Promise<void>((resolve) => {
    completeSave = resolve;
  });
  await page.route("**/api/review", async (route) => {
    const update = route.request().postDataJSON() as ReviewUpdateRequest;
    await saveReady;
    harness.data.images[0].selected_profile_index = update.selected_profile_index ?? 0;
    await route.fulfill({ json: harness.data });
  });
  await page.keyboard.press("PageDown");
  await expect(page.locator("#profile-state")).toContainText("Soft");
  await sendState(page, structuredClone(harness.data));
  await expect(page.locator("#profile-state")).toContainText("Soft");
  completeSave?.();
  await expect.poll(() => harness.data.images[0].selected_profile_index).toBe(1);
  await expect(page.locator("#profile-state")).toContainText("Soft");
  expect(harness.errors).toEqual([]);
});

test("retouch Escape restores the slider draft without saving it", async ({ page }) => {
  const harness = await openReview(page);
  const slider = page.locator("#retouch-exposure");
  await slider.focus();
  await slider.press("ArrowRight");
  await expect(slider).not.toHaveValue("0");
  await slider.press("Escape");
  await expect(slider).toHaveValue("0");
  expect(harness.data.images[0].retouch.adjustments.exposure).toBe(0);
  expect(harness.errors).toEqual([]);
});

test("sampler polling stops on close and ignores a late initial response", async ({ page }) => {
  const harness = await openReview(page);
  let polls = 0;
  await page.route(/\/api\/sampler\/jobs(?:\/|$)/, async (route) => {
    if (route.request().url().endsWith("/priority")) {
      await route.fulfill({ status: 204 });
      return;
    }
    if (route.request().method() === "GET") polls += 1;
    await route.fulfill({ json: { ...samplerFixture(), status: "rendering" } });
  });
  await page.locator("#sampler").click();
  await expect.poll(() => polls).toBeGreaterThan(0);
  await page.keyboard.press("Escape");
  const stopped = polls;
  await page.waitForTimeout(700);
  expect(polls).toBe(stopped);
  let releaseResponse: (() => void) | undefined;
  const responseReady = new Promise<void>((resolve) => {
    releaseResponse = resolve;
  });
  await page.route("**/api/sampler/jobs", async (route) => {
    await responseReady;
    await route.fulfill({ json: samplerFixture() });
  });
  await page.locator("#sampler").click();
  await expect(page.locator("#sampler-overlay")).toBeVisible();
  await page.keyboard.press("Escape");
  releaseResponse?.();
  await expect(page.locator("#sampler-overlay")).toBeHidden();
  expect(harness.errors).toEqual([]);
});

test("tool dialogs preserve profile details, command text, and rendered comparison layout", async ({ page }, info) => {
  const harness = await openReview(page);
  const pp3 = "[Version]\nAppVersion=5.12\n[Exposure]\nCompensation=0\n";
  await page.route("**/api/profile/0/pp3/1", async (route) => {
    await route.fulfill({ contentType: "text/plain", body: pp3 });
  });
  await page.locator(".current-profile-link").click();
  await page.getByText("Complete PP3", { exact: true }).click();
  await expect(page.locator(".profile-info-pp3")).toHaveText(pp3);
  await expect(page.locator(".profile-info-pp3-download")).toHaveAttribute("download", "frame-1--Classic.pp3");
  await compareBaseline(page, info, "profile-information");
  await page.keyboard.press("Escape");
  await page.locator("#app-version").click();
  await expect(page.locator("#command-invocation-overlay")).toBeVisible();
  await expect(page.locator("#command-invocation-overlay")).toContainText("Classic");
  await compareBaseline(page, info, "command-invocation");
  await page.keyboard.press("Escape");
  await page.locator("#sampler").click();
  await expect(page.locator("#sampler-overlay")).toContainText("Complete");
  await compareBaseline(page, info, "sampler-comparison");
  const section = page.locator(".sampler-section").first();
  await section.locator(":scope > summary").click();
  await sendState(page, structuredClone(harness.data));
  await expect(section).not.toHaveAttribute("open", "");
  await page.keyboard.press("Escape");
  await page.locator("#diffusion").click();
  await expect(page.getByRole("button", { name: "Apply to current", exact: true })).toBeEnabled();
  await compareBaseline(page, info, "diffusion-comparison");
  expect(harness.errors).toEqual([]);
});

test("publish controls preserve focus and draft text across unrelated server updates", async ({ page }, info) => {
  const harness = await openReview(page);
  await page.locator("#publish").click();
  const album = page.locator("#publish-album");
  await album.fill("My selected album");
  await album.focus();
  await sendState(page, { type: "patch", version: harness.data.version, client_count: 4 });
  await expect(album).toBeFocused();
  await expect(album).toHaveValue("My selected album");
  await page.locator("#publish-size-mode").selectOption("bounds");
  await page.locator("#publish-max-width").fill("3000");
  await page.locator("#publish-max-height").fill("2000");
  await expect(page.locator("#publish-long-edge")).toBeHidden();
  await expect(page.locator("#publish-resize")).toBeHidden();
  await page.locator("#publish-normalize-grain").uncheck();
  await expect(page.locator("#publish-normalize-grain-mpix")).toBeDisabled();
  await compareBaseline(page, info, "publish-draft");
  await page.locator("#publish-submit").click();
  await expect.poll(() => harness.requests.filter((request) => request.path === "publish").length).toBe(1);
  expect(harness.requests.find((request) => request.path === "publish")?.body).toMatchObject({
    album: "My selected album",
    size_mode: "bounds",
    max_width: 3000,
    max_height: 2000,
    normalize_grain: false,
  });
  expect(harness.errors).toEqual([]);
});

test("a stale diffusion response cannot replace a newer slider preview", async ({ page }) => {
  const harness = await openReview(page);
  let requestCount = 0;
  let releaseFirst: (() => void) | undefined;
  const firstReady = new Promise<void>((resolve) => {
    releaseFirst = resolve;
  });
  await page.route("**/api/diffusion/jobs", async (route) => {
    requestCount += 1;
    const id = requestCount;
    const body = route.request().postDataJSON() as { settings: DiffusionSettings };
    if (id === 1) await firstReady;
    await route.fulfill({ json: { ...diffusionFixture(), id, settings: body.settings } });
  });
  await page.locator("#diffusion").click();
  await expect.poll(() => requestCount).toBe(1);
  await page.locator("#diffusion-softness").fill("40");
  await expect.poll(() => requestCount).toBe(2);
  await expect(page.getByRole("button", { name: "Apply to current", exact: true })).toBeEnabled();
  releaseFirst?.();
  await expect(page.locator("#diffusion-softness")).toHaveValue("40");
  await page.getByRole("button", { name: "Apply to current", exact: true }).click();
  await expect(page.locator("#diffusion-overlay")).toBeHidden();
  expect(harness.requests.find((request) => request.path === "diffusion/settings")?.body).toMatchObject({
    settings: { softness: 40 },
  });
  expect(harness.errors).toEqual([]);
});
