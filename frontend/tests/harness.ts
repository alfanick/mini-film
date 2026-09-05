/** Isolate browser tests behind Rust-validated outgoing requests, HTTP fixtures, and a controllable event stream. */
import { expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { ReviewStateData, ReviewStateMessage } from "../review/core/types";
import { diffusionFixture, reviewFixture, samplerFixture } from "./fixtures";
import type { ReviewKeepalive } from "../review/generated/responses";
import { operations } from "../review/generated/operations";
import * as validators from "../review/generated/validators.mjs";
import { defaultRetouch } from "../review/core/selectors";

/** Keep every Rust-owned operation covered, including the four routes with no JSON request body. */
const requestValidators = {
  state: null,
  review: validators.validateRequestReview,
  ui: validators.validateRequestUi,
  burst: validators.validateRequestBurst,
  publish: validators.validateRequestPublish,
  sampler_create: validators.validateRequestSamplerCreate,
  sampler_get: null,
  sampler_priority: validators.validateRequestSamplerPriority,
  sampler_select: validators.validateRequestSamplerSelect,
  diffusion_create: validators.validateRequestDiffusionCreate,
  diffusion_get: null,
  diffusion_apply: validators.validateRequestDiffusionApply,
  diffusion_reset: validators.validateRequestDiffusionReset,
  panorama_create: validators.validateRequestPanoramaCreate,
  panorama_update: validators.validateRequestPanoramaUpdate,
  panorama_previews: validators.validateRequestPanoramaPreviews,
  panorama_render: validators.validateRequestPanoramaRender,
  events: null,
} satisfies Record<keyof typeof operations, ((body: unknown) => boolean) | null>;

/** Narrow reflective catalog keys without asserting types for an untrusted request. */
function isOperationName(name: string): name is keyof typeof operations {
  return Object.prototype.hasOwnProperty.call(operations, name);
}

/** Match one encoded URL segment per placeholder, leaving encoded slashes inside profile entry keys intact. */
function matchesRoute(template: string, path: string): boolean {
  const expected = template.split("/");
  const actual = `api/${path}`.split("/");
  return (
    expected.length === actual.length &&
    expected.every((segment, index) =>
      segment.startsWith("{") && segment.endsWith("}") ? Boolean(actual[index]) : segment === actual[index],
    )
  );
}

/** Reject incorrect methods, routes, missing bodies, and incompatible JSON before a fixture can accept them. */
export function validateOutgoingRequest(path: string, method: string, body: unknown): keyof typeof operations {
  for (const name of Object.keys(operations)) {
    if (!isOperationName(name)) continue;
    const operation = operations[name];
    if (operation.method !== method || !matchesRoute(operation.path, path)) continue;
    const validate = requestValidators[name];
    if (!operation.hasRequest) {
      if (body !== undefined) throw new Error(`Outgoing ${name} request must not contain a JSON body`);
    } else if (body === undefined && operation.allowEmptyRequest) {
      // These three handlers explicitly substitute their Rust defaults for an absent HTTP body.
    } else if (!validate || !validate(body)) {
      throw new Error(`Outgoing ${name} request does not satisfy its Rust JSON contract`);
    }
    return name;
  }
  throw new Error(`Outgoing ${method} api/${path} does not match a Rust JSON operation`);
}

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
    const path = new URL(request.url()).pathname.split("/api/")[1];
    if (path === undefined) return;
    // PP3 and publish-log downloads return text/files, not one of the JSON operations in the Rust catalog.
    if (request.method() === "GET" && (path.startsWith("profile/") || path.startsWith("publish/"))) return;
    const body: unknown = request.postData() ? request.postDataJSON() : undefined;
    // Observe every request so delayed/error response overrides cannot bypass this gate.
    validateOutgoingRequest(path, request.method(), body);
  });
  await page.addInitScript(() => {
    /** Deliver deterministic SSE messages while counting duplicate subscriptions. */
    class ReviewEvents extends EventTarget {
      onopen: (() => void) | null = null;
      onerror: (() => void) | null = null;
      onmessage: ((event: MessageEvent<string>) => void) | null = null;
      private readonly messageListener: (event: Event) => void;
      private readonly keepaliveListener: (event: Event) => void;
      private readonly reconnectListener: () => void;
      /** Emulate asynchronous connection and the browser's onmessage callback. */
      constructor() {
        super();
        document.documentElement.dataset["eventSources"] = String(
          Number(document.documentElement.dataset["eventSources"] || 0) + 1,
        );
        this.messageListener = (event): void => {
          if (event instanceof MessageEvent) {
            const data: unknown = event.data;
            if (typeof data === "string") this.onmessage?.(new MessageEvent<string>("message", { data }));
          }
        };
        this.keepaliveListener = (event): void => {
          if (event instanceof MessageEvent) {
            const data: unknown = event.data;
            if (typeof data === "string") this.dispatchEvent(new MessageEvent<string>("keepalive", { data }));
          }
        };
        this.reconnectListener = (): void => {
          this.onerror?.();
          this.onopen?.();
        };
        window.addEventListener("review-test-message", this.messageListener);
        window.addEventListener("review-test-keepalive", this.keepaliveListener);
        window.addEventListener("review-test-reconnect", this.reconnectListener);
        setTimeout(() => this.onopen?.(), 0);
      }
      /** Match EventSource cleanup when a mounted review application is disposed. */
      close(): void {
        this.onopen = null;
        this.onmessage = null;
        this.onerror = null;
        window.removeEventListener("review-test-message", this.messageListener);
        window.removeEventListener("review-test-keepalive", this.keepaliveListener);
        window.removeEventListener("review-test-reconnect", this.reconnectListener);
      }
    }
    Object.defineProperty(window, "EventSource", { value: ReviewEvents });
  });
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname.split("/api/")[1];
    if (path === undefined) throw new Error("Fixture intercepted a URL outside the review API");
    const body: unknown = request.postData() ? request.postDataJSON() : undefined;
    harness.requests.push({ path, method: request.method(), body });
    if (path === "review") {
      if (!validators.validateRequestReview(body)) throw new Error("Invalid review fixture request");
      const update = body;
      const image = harness.data.images.find((image) => image.id === update.image_id);
      if (image) {
        image.rating = update.rating;
        image.label = update.label ?? "none";
        image.labels = update.labels ?? [];
        image.tags = update.tags;
        image.notes = update.notes ?? "";
        if (update.retouch) {
          // Rust fills omitted request fields before serializing the required response representation.
          image.retouch = {
            adjustments: { ...defaultRetouch().adjustments, ...update.retouch.adjustments },
            crop: update.retouch.crop ? { x: 0, y: 0, width: 1, height: 1, ...update.retouch.crop } : null,
            rotation_degrees: update.retouch.rotation_degrees ?? 0,
          };
        }
        if (update.selected_profile_index != null) image.selected_profile_index = update.selected_profile_index;
        if (update.publish_profile_indexes) image.publish_profile_indexes = update.publish_profile_indexes;
        if (update.enabled_profile_indexes) {
          for (const profile of image.profiles)
            profile.enabled = update.enabled_profile_indexes.includes(profile.profile_index);
        }
        if (update.profile_bw_filters)
          image.profile_bw_filters = update.profile_bw_filters.map((entry) => ({
            profile_index: entry.profile_index,
            filter: entry.filter ?? "none",
          }));
        if (update.advance_after_update) harness.data.ui.current_image_id = Math.min(3, image.id + 1);
      }
    } else if (path === "ui") {
      if (!validators.validateRequestUi(body)) throw new Error("Invalid UI fixture request");
      Object.assign(harness.data.ui, body);
    } else if (path === "diffusion/settings") {
      // Settings endpoints acknowledge with a state patch, including DELETE.
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
    await route.fulfill({ json: path === "state" ? harness.data : { ...harness.data, type: "patch" } });
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

/** Exercise named SSE keepalive messages independently of snapshot delivery. */
export async function sendKeepalive(page: Page, message: ReviewKeepalive): Promise<void> {
  await page.evaluate(
    (data) => window.dispatchEvent(new MessageEvent("review-test-keepalive", { data: JSON.stringify(data) })),
    message,
  );
}

/** Exercise a recoverable EventSource disconnect followed by the browser's reconnect event. */
export async function reconnect(page: Page): Promise<void> {
  await page.evaluate(() => window.dispatchEvent(new Event("review-test-reconnect")));
}

/** Send snapshots or patches through the same callback used by EventSource. */
export async function sendState(page: Page, message: ReviewStateMessage): Promise<void> {
  await page.evaluate(
    (data) => window.dispatchEvent(new MessageEvent("review-test-message", { data: JSON.stringify(data) })),
    message,
  );
}
