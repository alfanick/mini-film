/** Deterministic lifecycle and publish barriers prove field ownership without trusting browser unload delivery. */
import { expect, test } from "@playwright/test";
import { ReviewDraftModel, type DraftPorts } from "../review/session/draft-model";
import { createCommandQueue } from "../review/session/commands";
import { invalidatedOutputs } from "../review/session/barriers";
import { publishOutputsReady, publishWaitSet } from "../review/features/publish/barrier";
import { publishBody } from "../review/features/publish/model";
import { PublishModel } from "../review/features/publish/model";
import { ReviewModel } from "../review/core/model";
import type { ToolSessionActions } from "../review/tools/types";
import type { ReviewLabel } from "../review/core/types";
import type { PublishDraft } from "../review/features/publish/model";
import { createState } from "../review/core/state";
import { reviewFixture } from "./fixtures";

/** Supply controllable completion without timers or a browser process. */
function deferred(): { promise: Promise<void>; resolve: () => void } {
  let complete = (): void => {
    throw new Error("Uninitialized deferred completion");
  };
  const promise = new Promise<void>((resolve) => {
    complete = resolve;
  });
  return { promise, resolve: (): void => complete() };
}

/** Isolate draft ownership with a real fixture and an injected save function. */
function drafts(save: DraftPorts["save"]): InstanceType<typeof ReviewDraftModel> {
  const catalog = reviewFixture();
  return new ReviewDraftModel({
    findImage: (id) => catalog.images.find((image) => image.id === id) || null,
    save,
    visibleRetouch: (_image, value) => value,
    presentRetouch: (): void => undefined,
    schedule: (): (() => void) => (): void => undefined,
  });
}

test("lifecycle flush excludes clean focus and duplicate in-flight revisions", async (): Promise<void> => {
  const pending = deferred();
  const saves: Parameters<DraftPorts["save"]>[] = [];
  const model = drafts((...args): Promise<void> => {
    saves.push(args);
    return pending.promise;
  });
  try {
    model.focusMetadata(1, "tags");
    await model.flushOwned(undefined, true);
    expect(saves).toHaveLength(0);
    model.setMetadata(1, "notes", "Owned at hidden");
    const hidden = model.flushOwned(undefined, true);
    await model.flushOwned(undefined, true);
    expect(saves).toHaveLength(1);
    expect(saves[0]?.[1]).toEqual({ notes: "Owned at hidden" });
    expect(saves[0]?.[2]).toEqual({ automatic: true, keepalive: true });
    pending.resolve();
    await hidden;
  } finally {
    model[Symbol.dispose]();
  }
});

test("a later automatic sibling edit never retries an earlier failed field", async (): Promise<void> => {
  const saves: Parameters<DraftPorts["save"]>[] = [];
  const model = drafts((...args): Promise<void> => {
    saves.push(args);
    return saves.length === 1 ? Promise.reject(new Error("Lost acknowledgement")) : Promise.resolve();
  });
  try {
    model.setMetadata(1, "tags", "failed tag");
    await expect(model.flushOwned()).rejects.toThrow("Lost acknowledgement");
    model.setMetadata(1, "notes", "new independent note");
    await model.flushOwned();
    expect(saves[1]?.[1]).toEqual({ notes: "new independent note" });
    expect(model.errors.peek()).toEqual([{ imageId: 1, message: "Lost acknowledgement" }]);
    await model.flush(1);
    expect(saves[2]?.[1]).toEqual({ tags: ["failed", "tag"] });
    expect(model.errors.peek()).toEqual([]);
  } finally {
    model[Symbol.dispose]();
  }
});

test("a fixed barrier holds later edits until its job snapshot", async (): Promise<void> => {
  const queue = createCommandQueue();
  const commit = deferred();
  const snapshot = deferred();
  const order: string[] = [];
  const first = queue.enqueue(async (): Promise<void> => {
    order.push("edit");
    await commit.promise;
  });
  const job = queue.enqueue(async (): Promise<number> => {
    order.push("publish");
    await snapshot.promise;
    return 12;
  });
  const later = queue.enqueue((): Promise<void> => {
    order.push("later edit");
    return Promise.resolve();
  });
  await Promise.resolve();
  expect(order).toEqual(["edit"]);
  commit.resolve();
  await first;
  await Promise.resolve();
  expect(order).toEqual(["edit", "publish"]);
  snapshot.resolve();
  expect(await job).toBe(12);
  await later;
  expect(order).toEqual(["edit", "publish", "later edit"]);
});

/** Use complete textual form values without inventing an alternate request contract. */
function form(): PublishDraft {
  return {
    album: "test",
    minRating: "0",
    labels: [],
    tags: "",
    mainProfileOnly: false,
    outputFormat: "jpg",
    grainEngine: "legacy",
    normalizeGrain: false,
    normalizeGrainMpix: "12",
    sizeMode: "original",
    longEdge: "",
    maxWidth: "",
    maxHeight: "",
    resize: "",
    jpgQuality: "95",
    jpegSubsampling: "s444",
    progressive: false,
    stripMetadata: false,
    gallery: "none",
    galleryColumns: "3",
    galleryThumbnailLongEdge: "1024",
  };
}

test("publish waits only selected outputs invalidated by its own fixed edit cut", (): void => {
  const data = reviewFixture();
  const image = data.images[0];
  if (!image) throw new Error("Fixture image missing");
  const after = { ...image, retouch: { ...image.retouch, rotation_degrees: 1 } };
  const affected = invalidatedOutputs(image, after);
  expect(affected.length).toBeGreaterThan(0);
  expect(invalidatedOutputs(image, { ...image, notes: "metadata only" })).toEqual([]);
  const snapshot = { ...createState(), data };
  const context = {
    affectedOutputs: new Set(affected),
    snapshot,
    refresh: (): Promise<typeof snapshot> => Promise.resolve(snapshot),
  };
  const targets = publishWaitSet(context, publishBody(form()));
  expect(targets.every((key) => key.startsWith(`${image.id}:`))).toBe(true);
  const render = image.profiles.find((profile) => targets.includes(`${image.id}:${profile.profile_index}`));
  if (!render) throw new Error("Fixture selected output missing");
  render.status = "queued";
  render.retouch_pending = true;
  expect(publishOutputsReady(snapshot, targets)).toBe(false);
  render.status = "done";
  render.retouch_pending = false;
  expect(publishOutputsReady(snapshot, targets)).toBe(true);
  render.status = "failed";
  expect(() => publishOutputsReady(snapshot, targets)).toThrow("render failed");
});

/** Supply model-owned catalog observations without creating browser listeners or a second mutation queue. */
function publishHarness(): {
  catalog: InstanceType<typeof ReviewModel>;
  publish: InstanceType<typeof PublishModel>;
} {
  const catalog = new ReviewModel();
  catalog.applyMessage(reviewFixture());
  const session: ToolSessionActions = {
    applyMessage: catalog.applyMessage,
    updateSharedUi: (): Promise<void> => Promise.resolve(),
    commit: (_scope, operation) =>
      operation({
        affectedOutputs: new Set(),
        snapshot: catalog.getConfirmedState(),
        refresh: () => Promise.resolve(catalog.getConfirmedState()),
      }),
  };
  return { catalog, publish: new PublishModel(catalog, session) };
}

test("publish input ownership and request capture cannot be changed through caller arrays", (): void => {
  const { catalog, publish } = publishHarness();
  try {
    const labels: ReviewLabel[] = ["red"];
    publish.setPublishField("labels", labels);
    labels.push("blue");
    expect(publish.publishForm.peek().labels).toEqual(["red"]);
    const body = publishBody(publish.publishForm.peek());
    body.labels.push("green");
    expect(publish.publishForm.peek().labels).toEqual(["red"]);
  } finally {
    publish[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("ambiguous publish recovery checks state and resumes without replaying the POST", async (): Promise<void> => {
  const previousFetch = globalThis.fetch;
  const { catalog, publish } = publishHarness();
  let posts = 0;
  globalThis.fetch = (): Promise<Response> => {
    posts += 1;
    return Promise.resolve(new Response(JSON.stringify({ error: "Acknowledgement lost" }), { status: 503 }));
  };
  try {
    const submission = publish.submitPublish();
    await expect.poll(() => publish.publishRecovery.peek()).toBe(true);
    expect(posts).toBe(1);
    publish.togglePublishWizard(false);
    expect(publish.publishSubmitting.peek()).toBe(true);
    await publish.submitPublish();
    await submission;
    expect(posts).toBe(1);
    expect(publish.publishRecovery.peek()).toBe(false);
    expect(publish.publishError.peek()).toContain("outcome unknown");
  } finally {
    globalThis.fetch = previousFetch;
    publish[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});
