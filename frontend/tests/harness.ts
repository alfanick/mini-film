/** Isolate review browser tests behind typed HTTP fixtures and a controllable server-event stream. */
import { expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { ReviewStateData, ReviewStateMessage, ReviewUpdateRequest, ReviewUiState } from "../review/core/types";
import { diffusionFixture, reviewFixture, samplerFixture } from "./fixtures";

interface RecordedRequest {
  path: string;
  method: string;
  body: unknown;
}
interface ReviewHarness {
  data: ReviewStateData;
  requests: RecordedRequest[];
  errors: string[];
  scripts: string[];
}

/** Mount the real UI with isolated HTTP fixtures and a controllable SSE source. */
export async function openReview(page: Page): Promise<ReviewHarness> {
  const harness: ReviewHarness = { data: reviewFixture(), requests: [], errors: [], scripts: [] };
  page.on("pageerror", (error) => harness.errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") harness.errors.push(message.text());
  });
  page.on("request", (request) => {
    if (request.resourceType() === "script") harness.scripts.push(request.url());
  });
  await page.addInitScript(() => {
    /** Deliver deterministic SSE messages while counting duplicate subscriptions. */
    class ReviewEvents extends EventTarget {
      onopen: (() => void) | null = null;
      onmessage: ((event: MessageEvent<string>) => void) | null = null;
      /** Emulate asynchronous connection and the browser's onmessage callback. */
      constructor() {
        super();
        document.documentElement.dataset.eventSources = String(
          Number(document.documentElement.dataset.eventSources || 0) + 1,
        );
        window.addEventListener("review-test-message", (event) => {
          if (event instanceof MessageEvent) {
            const data: unknown = event.data;
            if (typeof data === "string") this.onmessage?.(new MessageEvent<string>("message", { data }));
          }
        });
        setTimeout(() => this.onopen?.(), 0);
      }
      /** Match EventSource cleanup when a mounted review application is disposed. */
      close(): void {
        this.onopen = null;
        this.onmessage = null;
      }
    }
    Object.defineProperty(window, "EventSource", { value: ReviewEvents });
  });
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname.split("/api/")[1];
    const body: unknown = request.postData() ? request.postDataJSON() : undefined;
    harness.requests.push({ path, method: request.method(), body });
    if (path === "review") {
      const update = body as ReviewUpdateRequest;
      const image = harness.data.images.find((image) => image.id === update.image_id);
      if (image) {
        image.rating = update.rating;
        image.label = update.label;
        image.labels = update.labels;
        image.tags = update.tags;
        image.notes = update.notes;
        if (update.retouch) image.retouch = update.retouch;
        if (update.selected_profile_index !== undefined) image.selected_profile_index = update.selected_profile_index;
        if (update.publish_profile_indexes) image.publish_profile_indexes = update.publish_profile_indexes;
        if (update.advance_after_update) harness.data.ui.current_image_id = Math.min(3, image.id + 1);
      }
    } else if (path === "ui") {
      Object.assign(harness.data.ui, body as ReviewUiState);
    } else if (path === "diffusion/settings" || path.endsWith("/priority")) {
      await route.fulfill({ status: 204 });
      return;
    } else if (path.startsWith("sampler/jobs")) {
      await route.fulfill({ json: samplerFixture() });
      return;
    } else if (path.startsWith("diffusion/jobs")) {
      await route.fulfill({ json: diffusionFixture() });
      return;
    } else if (path === "publish") {
      harness.data.publish_jobs.push({
        id: 11,
        album: "Album test",
        status: "done",
        started_at: "2026-09-05T10:00:00Z",
        finished_at: "2026-09-05T10:00:01Z",
        processed: 3,
        total: 3,
        step: "Done",
        current: null,
        linked: 3,
        skipped: 0,
        galleries: 1,
        gallery_urls: [],
        error: null,
      });
    } else if (path !== "state") {
      await route.fulfill({ status: 404, json: { error: `Unexpected fixture request: ${path}` } });
      return;
    }
    await route.fulfill({ json: harness.data });
  });
  await page.goto("./");
  await expect(page.locator("#image-title")).toHaveText("frame-1.NEF");
  await expect(page.locator("#live-dot")).toHaveClass(/connected/);
  await expect
    .poll(() =>
      page.locator("#main-image").evaluate((image: HTMLImageElement) => image.complete && image.naturalWidth > 0),
    )
    .toBe(true);
  return harness;
}

/** Send snapshots or patches through the same callback used by EventSource. */
export async function sendState(page: Page, message: ReviewStateMessage): Promise<void> {
  await page.evaluate(
    (data) => window.dispatchEvent(new MessageEvent("review-test-message", { data: JSON.stringify(data) })),
    message,
  );
}
