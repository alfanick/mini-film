/** Prove local command ordering, per-image draft ownership and fine-grained subscriptions without browser timing. */
import { expect, test } from "@playwright/test";
import { effect } from "@preact/signals";
import { ReviewModel } from "../review/core/model";
import { ReviewDraftModel, fieldDirty, type DraftPorts } from "../review/session/draft-model";
import { createFailureTracker } from "../review/session/failures";
import {
  createCommandQueue,
  projectReviewIntent,
  reviewIntentFields,
  type ReviewIntent,
} from "../review/session/commands";
import type { ReviewImage, ReviewUpdateRequest } from "../review/core/types";
import { reviewFixture } from "./fixtures";

/** Require a fixture value while narrowing both absent entries and explicitly nullable model selections. */
function required<T>(value: T): NonNullable<T> {
  if (value === null || value === undefined) throw new Error("Model fixture value is absent");
  return value;
}

/** Create a deterministic promise boundary for acknowledgements that arrive after further edits. */
function deferred(): { promise: Promise<void>; resolve: () => void; reject: (error: Error) => void } {
  let resolve: () => void = (): void => {
    throw new Error("Deferred promise was not initialized");
  };
  let reject: (error: Error) => void = (): void => {
    throw new Error("Deferred promise was not initialized");
  };
  const promise = new Promise<void>((yes, no): void => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

/** Supply real models with an isolated catalog, recorded requests, and explicit timer callbacks. */
function draftHarness(): {
  model: InstanceType<typeof ReviewModel>;
  drafts: InstanceType<typeof ReviewDraftModel>;
  saves: { id: number; fields: Partial<ReviewUpdateRequest> }[];
  timers: Map<number, { delay: number; fire: () => void }>;
  setSave: (save: DraftPorts["save"]) => void;
  dispose: () => void;
} {
  const model = new ReviewModel();
  model.applyMessage(reviewFixture());
  const saves: { id: number; fields: Partial<ReviewUpdateRequest> }[] = [];
  const timers = new Map<number, { delay: number; fire: () => void }>();
  let timerId = 0;
  let save: DraftPorts["save"] = (image, fields): Promise<void> => {
    const data = required(model.getConfirmedState().data);
    model.applyMessage({
      ...data,
      images: data.images.map((item) =>
        item.id === image.id ? projectReviewIntent(item, { kind: "fields", fields }) : item,
      ),
    });
    return Promise.resolve();
  };
  const drafts = new ReviewDraftModel({
    findImage: (id): ReviewImage | null =>
      model.getConfirmedState().data?.images.find((image) => image.id === id) || null,
    save: (image, fields): Promise<void> => {
      saves.push({ id: image.id, fields });
      return save(image, fields);
    },
    visibleRetouch: (_image, value) => value,
    presentRetouch: model.setRetouchDraft,
    schedule: (fire, delay): (() => void) => {
      const id = ++timerId;
      timers.set(id, { fire, delay });
      return (): void => {
        timers.delete(id);
      };
    },
  });
  return {
    model,
    drafts,
    saves,
    timers,
    setSave(next): void {
      save = next;
    },
    dispose(): void {
      drafts[Symbol.dispose]();
      model[Symbol.dispose]();
    },
  };
}

test("shared navigation never replaces another image's unsaved fields or timer", async (): Promise<void> => {
  const harness = draftHarness();
  try {
    harness.drafts.setMetadata(1, "notes", "A survives");
    harness.model.update({ currentId: 2 });
    harness.drafts.setMetadata(2, "notes", "B stays separate");
    expect(harness.timers.size).toBe(2);
    expect(Array.from(harness.timers.values()).map((timer) => timer.delay)).toEqual([500, 500]);
    await Promise.all([harness.drafts.flush(1), harness.drafts.flush(2)]);
    expect(harness.saves.map((save) => [save.id, save.fields.notes])).toEqual([
      [1, "A survives"],
      [2, "B stays separate"],
    ]);
    expect(harness.timers.size).toBe(0);
  } finally {
    harness.dispose();
  }
});

test("an older acknowledgement clears only the field revisions it submitted", async (): Promise<void> => {
  const harness = draftHarness();
  const first = deferred();
  harness.setSave(() => first.promise);
  try {
    harness.drafts.setMetadata(1, "notes", "First");
    const saving = harness.drafts.flush(1);
    harness.drafts.setMetadata(1, "notes", "Second");
    first.resolve();
    await saving;
    const pending = required(harness.drafts.image(1).peek());
    expect(pending.notes.value).toBe("Second");
    expect(fieldDirty(pending.notes)).toBe(true);
    await harness.drafts.flush(1);
    expect(harness.saves.map((save) => save.fields.notes)).toEqual(["First", "Second"]);
  } finally {
    harness.dispose();
  }
});

test("unrelated server patches cannot acknowledge or hide a local retouch preview", async (): Promise<void> => {
  const harness = draftHarness();
  try {
    const retouch = structuredClone(required(harness.model.image(1).peek()).retouch);
    retouch.adjustments.exposure = 1;
    harness.drafts.setRetouch(1, retouch);
    expect(Array.from(harness.timers.values()).map((timer) => timer.delay)).toEqual([1200]);
    harness.model.applyMessage({ type: "patch", version: "22.15.1", client_count: 3 });
    expect(harness.model.field("localRetouchDirty").peek()).toBe(true);
    expect(required(harness.model.image(1).peek()).retouch.adjustments.exposure).toBe(1);
    expect(required(harness.model.getConfirmedState().data?.images[0]).retouch.adjustments.exposure).toBe(0);
    await harness.drafts.flush(1);
    expect(harness.model.field("localRetouchDirty").peek()).toBe(false);
  } finally {
    harness.dispose();
  }
});

test("failed saves retain draft values and do not retry themselves", async (): Promise<void> => {
  const harness = draftHarness();
  harness.setSave(() => Promise.reject(new Error("Camera store unavailable")));
  try {
    harness.drafts.setMetadata(1, "notes", "Keep this note");
    await expect(harness.drafts.flush(1)).rejects.toThrow("Camera store unavailable");
    expect(harness.saves).toHaveLength(1);
    expect(harness.timers.size).toBe(0);
    expect(harness.drafts.errors.peek()).toEqual([{ imageId: 1, message: "Camera store unavailable" }]);
    expect(required(harness.drafts.image(1).peek()).notes.value).toBe("Keep this note");
    harness.setSave(() => Promise.resolve());
    await harness.drafts.flush(1);
    expect(harness.saves).toHaveLength(2);
    expect(harness.drafts.errors.peek()).toEqual([]);
  } finally {
    harness.dispose();
  }
});

test("repeated flushes share an already in-flight revision set", async (): Promise<void> => {
  const harness = draftHarness();
  const response = deferred();
  harness.setSave(() => response.promise);
  try {
    harness.drafts.setMetadata(1, "tags", "007, 007");
    const first = harness.drafts.flush(1);
    const second = harness.drafts.flush(1);
    expect(harness.saves).toHaveLength(1);
    expect(harness.saves[0]?.fields.tags).toEqual(["007", "007"]);
    response.resolve();
    await Promise.all([first, second]);
  } finally {
    harness.dispose();
  }
});

test("focused retouch remains explicitly committable after autosave and a server update", async (): Promise<void> => {
  const harness = draftHarness();
  try {
    harness.drafts.focusRetouch(1, true);
    const retouch = structuredClone(required(harness.model.image(1).peek()).retouch);
    retouch.adjustments.exposure = 0.8;
    harness.drafts.setRetouch(1, retouch, false);
    await harness.drafts.flush(1);
    const remote = structuredClone(required(harness.model.getConfirmedState().data));
    required(remote.images[0]).retouch.adjustments.exposure = 2;
    harness.model.applyMessage(remote);
    expect(required(harness.drafts.image(1).peek()).retouch.value.adjustments.exposure).toBe(0.8);
    await harness.drafts.flush(1, true);
    expect(harness.saves[1]?.fields.retouch?.adjustments.exposure).toBe(0.8);
  } finally {
    harness.dispose();
  }
});

test("semantic commands compile against the acknowledged result of prior local commands", async (): Promise<void> => {
  const model = new ReviewModel();
  model.applyMessage(reviewFixture());
  const queue = createCommandQueue();
  const bodies: number[][] = [];
  /** Match the session's intention boundary while keeping transport deterministic. */
  function execute(intent: ReviewIntent): Promise<void> {
    const id = model.beginCommand(1, intent);
    return queue.enqueue((): Promise<void> => {
      const data = required(model.getConfirmedState().data);
      const image = required(data.images[0]);
      const fields = reviewIntentFields(image, intent);
      bodies.push(fields.enabled_profile_indexes || []);
      model.applyMessage({ ...data, images: [projectReviewIntent(image, intent), ...data.images.slice(1)] });
      model.finishCommand(id);
      return Promise.resolve();
    });
  }
  try {
    await Promise.all([
      execute({ kind: "profile-enabled", profileIndex: 0, enabled: false }),
      execute({ kind: "profile-enabled", profileIndex: 1, enabled: false }),
    ]);
    expect(bodies).toEqual([[1], []]);
    expect(required(model.image(1).peek()).profiles.map((profile) => profile.enabled)).toEqual([false, false]);
  } finally {
    model[Symbol.dispose]();
  }
});

test("a rejected command does not poison independent later queue entries", async (): Promise<void> => {
  const queue = createCommandQueue();
  const first = queue.enqueue(() => Promise.reject(new Error("failed")));
  const performed: string[] = [];
  const second = queue.enqueue(() => {
    performed.push("second");
    return Promise.resolve();
  });
  await expect(first).rejects.toThrow("failed");
  await second;
  expect(performed).toEqual(["second"]);
});

test("one image patch notifies only its image subscriber and preserves untouched identities", (): void => {
  const model = new ReviewModel();
  model.applyMessage(reviewFixture());
  const first = model.image(1).peek();
  let firstReads = 0;
  let secondReads = 0;
  const stopFirst = effect(() => {
    void model.image(1).value;
    firstReads += 1;
  });
  const stopSecond = effect(() => {
    void model.image(2).value;
    secondReads += 1;
  });
  try {
    const changed = { ...required(model.image(2).peek()), notes: "New second note" };
    model.applyMessage({ type: "patch", version: "22.15.1", images: [changed] });
    expect(model.image(1).peek()).toBe(first);
    expect([firstReads, secondReads]).toEqual([1, 2]);
    model.applyMessage({ type: "patch", version: "22.15.1", client_count: 8 });
    expect([firstReads, secondReads]).toEqual([1, 2]);
  } finally {
    stopFirst();
    stopSecond();
    model[Symbol.dispose]();
  }
});

test("tool draft changes do not invalidate catalog, filtered rows or unrelated field subscriptions", (): void => {
  const model = new ReviewModel();
  model.applyMessage(reviewFixture());
  let catalogs = 0;
  let rows = 0;
  let selections = 0;
  const stopCatalog = effect(() => {
    void model.catalog.value;
    catalogs += 1;
  });
  const stopRows = effect(() => {
    void model.visibleImages.value;
    rows += 1;
  });
  const stopSelection = effect(() => {
    void model.field("currentId").value;
    selections += 1;
  });
  try {
    model.update({ panoramaName: "New project name", diffusionMessage: "Loading preview" });
    expect([catalogs, rows, selections]).toEqual([1, 1, 1]);
  } finally {
    stopCatalog();
    stopRows();
    stopSelection();
    model[Symbol.dispose]();
  }
});

test("separate provider models never share edits, pending selections or tool state", (): void => {
  const first = new ReviewModel();
  const second = new ReviewModel();
  try {
    first.applyMessage(reviewFixture());
    second.applyMessage(reviewFixture());
    first.beginCommand(1, { kind: "profile-selected", profileIndex: 1 });
    first.update({ samplerOpen: true });
    expect(required(first.image(1).peek()).selected_profile_index).toBe(1);
    expect(required(second.image(1).peek()).selected_profile_index).toBe(0);
    expect(second.field("samplerOpen").peek()).toBe(false);
  } finally {
    first[Symbol.dispose]();
    second[Symbol.dispose]();
  }
});

for (const newerFailed of [false, true]) {
  test(`old draft completion preserves newer ${newerFailed ? "failure" : "success"}`, async (): Promise<void> => {
    const harness = draftHarness();
    const older = deferred();
    const newer = deferred();
    let requests = 0;
    harness.setSave(() => (++requests === 1 ? older.promise : newer.promise));
    try {
      harness.drafts.setMetadata(1, "notes", "Older");
      const first = harness.drafts.flush(1).catch(() => undefined);
      harness.drafts.setMetadata(1, "notes", "Newer");
      const second = harness.drafts.flush(1).catch(() => undefined);
      if (newerFailed) newer.reject(new Error("Newest failure"));
      else newer.resolve();
      await second;
      if (newerFailed) older.resolve();
      else older.reject(new Error("Obsolete failure"));
      await first;
      expect(harness.drafts.errors.peek()).toEqual(newerFailed ? [{ imageId: 1, message: "Newest failure" }] : []);
    } finally {
      harness.dispose();
    }
  });
}

test("a removed image retains a visible unsaved draft instead of writing another picture", async (): Promise<void> => {
  const harness = draftHarness();
  try {
    harness.drafts.setMetadata(1, "notes", "Retain after removal");
    const data = required(harness.model.getConfirmedState().data);
    harness.model.applyMessage({ ...data, images: data.images.filter((image) => image.id !== 1) });
    await expect(harness.drafts.flush(1)).rejects.toThrow("unsaved edits have been retained");
    expect(required(harness.drafts.image(1).peek()).notes.value).toBe("Retain after removal");
    expect(harness.drafts.errors.peek()[0]?.message).toContain("unsaved edits have been retained");
    expect(harness.saves).toEqual([]);
  } finally {
    harness.dispose();
  }
});

test("semantic recovery ignores obsolete completions and never replays rating-and-advance", (): void => {
  const tracker = createFailureTracker();
  const older = tracker.begin(1, { kind: "profile-selected", profileIndex: 1 });
  const newer = tracker.begin(1, { kind: "profile-enabled", profileIndex: 0, enabled: false });
  tracker.fail(newer, "Current failure");
  tracker.fail(older, "Old failure");
  tracker.clear(older);
  expect(tracker.failures.peek()).toEqual([{ ...newer, message: "Current failure", retryable: true }]);
  tracker.clear(newer);
  tracker.fail(older, "Even later old failure");
  expect(tracker.failures.peek()).toEqual([]);
  const advance = tracker.begin(2, { kind: "fields", fields: { rating: 4, advance_after_update: true } });
  tracker.fail(advance, "Lost acknowledgement");
  expect(tracker.failures.peek()[0]?.retryable).toBe(false);
});

test("unrelated semantic domains retain independent failures and explicit recovery intentions", (): void => {
  const tracker = createFailureTracker();
  const profile = tracker.begin(1, { kind: "profile-enabled", profileIndex: 0, enabled: false });
  tracker.fail(profile, "Profile response lost");
  const label = tracker.begin(1, { kind: "label", label: "red", enabled: true });
  tracker.clear(label);
  const rating = tracker.begin(1, { kind: "fields", fields: { rating: 3 } });
  tracker.fail(rating, "Rating response lost");
  const filter = tracker.begin(1, { kind: "bw-filter", profileIndex: 0, filter: "red" });
  tracker.fail(filter, "Filter response lost");
  tracker.clear(tracker.begin(1, { kind: "bw-filter", profileIndex: 1, filter: "yellow" }));
  expect(tracker.failures.peek().map((failure) => failure.key)).toEqual(["1:profiles", "1:rating", "1:bw:0"]);
  tracker.clear(rating);
  expect(tracker.failures.peek().map((failure) => failure.key)).toEqual(["1:profiles", "1:bw:0"]);
});

for (const count of [1000, 10000]) {
  test(`${count} pictures retain narrow subscriptions for local edits and unrelated tool changes`, (): void => {
    const model = new ReviewModel();
    const fixture = reviewFixture();
    const template = required(fixture.images[0]);
    fixture.images = Array.from({ length: count }, (_unused, index) => ({ ...template, id: index + 1 }));
    model.applyMessage(fixture);
    let notifications = 0;
    const stops = fixture.images.map((image) =>
      effect(() => {
        void model.image(image.id).value;
        notifications += 1;
      }),
    );
    try {
      notifications = 0;
      const started = performance.now();
      model.update({ panoramaName: "No image subscription", diffusionMessage: "No catalog work" });
      expect(notifications).toBe(0);
      const retouch = structuredClone(template.retouch);
      retouch.adjustments.exposure = 1;
      model.setRetouchDraft(1, retouch);
      const elapsed = performance.now() - started;
      expect(notifications).toBe(1);
      expect(model.image(2).peek()).toBe(fixture.images[1]);
      test.info().annotations.push({ type: "performance", description: `${count} images: ${elapsed.toFixed(2)} ms` });
      console.info(`${count} images: edit + tool update ${elapsed.toFixed(2)} ms, ${notifications} image notification`);
    } finally {
      for (const stop of stops) stop();
      model[Symbol.dispose]();
    }
  });
}
