/** Exercise durable tool subscriptions and delayed response ownership without browser rendering or external engines. */
import { expect, test } from "@playwright/test";
import { effect } from "@preact/signals";
import { ReviewModel } from "../review/core/model";
import { InformationModel } from "../review/features/information/model";
import { PanoramaModel } from "../review/features/panorama/model";
import { DiffusionModel } from "../review/features/diffusion/model";
import type { ReviewPanoramaProject } from "../review/core/types";
import type { ToolSessionActions } from "../review/tools/types";
import { diffusionFixture, reviewFixture } from "./fixtures";
import { required } from "./required";

/** Hold one response until a test has changed the dialog or its draft. */
function delayedResponse(): { response: Promise<Response>; resolve: (value: Response) => void } {
  let resolve: (value: Response) => void = (): void => {
    throw new Error("Response boundary was not initialized");
  };
  const response = new Promise<Response>((complete): void => {
    resolve = complete;
  });
  return { response, resolve };
}

/** Hold a command barrier independently of transport so tests can close its dialog before it starts. */
function delayedBarrier(): { pending: Promise<void>; release: () => void } {
  let release: () => void = (): void => {
    throw new Error("Barrier boundary was not initialized");
  };
  const pending = new Promise<void>((complete): void => {
    release = complete;
  });
  return { pending, release };
}

/** Run the model's operation against a deterministic confirmed cut after an optional simulated save queue. */
function immediateCommit(
  catalog: InstanceType<typeof ReviewModel>,
  before?: Promise<void>,
): ToolSessionActions["commit"] {
  return async (_scope, operation) => {
    await before;
    return operation({
      snapshot: catalog.getConfirmedState(),
      affectedOutputs: new Set(),
      refresh: () => Promise.resolve(catalog.getConfirmedState()),
    });
  };
}

/** Construct a real wire project so response validators exercise created-ID ownership rather than accepting stubs. */
function project(id: number): ReviewPanoramaProject {
  return {
    id,
    name: `Project ${id}`,
    image_ids: [1, 2, 3],
    matching_mode: "automatic",
    selected_projection: "cylindrical",
    status: "draft",
    error: null,
    previews: [],
    progress_completed: 0,
    progress_total: 0,
    progress_stage: null,
    output_file_name: null,
    result_image_id: null,
    created_at: "2026-09-10T12:00:00Z",
    updated_at: "2026-09-10T12:00:00Z",
  };
}

test("closed tool state ignores unrelated catalog and client changes", (): void => {
  const catalog = new ReviewModel();
  const fixture = reviewFixture();
  catalog.applyMessage(fixture);
  const information = new InformationModel(catalog);
  const panorama = new PanoramaModel(catalog, {
    applyMessage: catalog.applyMessage,
    updateSharedUi: (): Promise<void> => Promise.resolve(),
  });
  const diffusion = new DiffusionModel(catalog, {
    applyMessage: catalog.applyMessage,
    commit: immediateCommit(catalog),
  });
  let informationReads = 0;
  let panoramaReads = 0;
  let diffusionReads = 0;
  const stopInformation = effect((): void => {
    void information.state.value;
    informationReads += 1;
  });
  const stopPanorama = effect((): void => {
    void panorama.state.value;
    panoramaReads += 1;
  });
  const stopDiffusion = effect((): void => {
    void diffusion.state.value;
    diffusionReads += 1;
  });
  try {
    catalog.update({ histogramOpen: true });
    catalog.applyMessage({ type: "patch", version: fixture.version, client_count: 8 });
    expect([informationReads, panoramaReads, diffusionReads]).toEqual([1, 1, 1]);
    information.openProfileInfo(required(fixture.profiles[0]));
    const image = required(fixture.images[0]);
    catalog.applyMessage({ type: "patch", version: fixture.version, images: [{ ...image, notes: "Live metadata" }] });
    expect(information.state.peek().image?.notes).toBe("Live metadata");
  } finally {
    stopInformation();
    stopPanorama();
    stopDiffusion();
    information[Symbol.dispose]();
    panorama[Symbol.dispose]();
    diffusion[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("closing information rejects a late PP3 response even when transport ignores abort", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const pending = delayedResponse();
  globalThis.fetch = (): Promise<Response> => pending.response;
  const fixture = reviewFixture();
  const catalog = new ReviewModel();
  catalog.applyMessage(fixture);
  const information = new InformationModel(catalog);
  try {
    const profile = required(fixture.profiles[0]);
    information.openProfileInfo(profile);
    const request = information.loadProfilePp3(required(fixture.images[0]), profile);
    expect(information.state.peek().profileInfoPp3.status).toBe("loading");
    information.closeProfileInfo();
    pending.resolve(new Response("[Exposure]\nCompensation=1"));
    await request;
    expect(information.state.peek().profileInfoPp3.status).toBe("idle");
    expect(information.state.peek().profileInfoProfileIndex).toBeNull();
  } finally {
    globalThis.fetch = originalFetch;
    information[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("panorama single flight honors its created ID and retains newer draft edits", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const pending = delayedResponse();
  const paths: string[] = [];
  const fixture = reviewFixture();
  globalThis.fetch = (input): Promise<Response> => {
    const path = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    paths.push(path);
    return path.endsWith("/panoramas")
      ? pending.response
      : Promise.resolve(Response.json({ type: "patch", version: fixture.version }));
  };
  const catalog = new ReviewModel();
  catalog.applyMessage(fixture);
  const panorama = new PanoramaModel(catalog, {
    applyMessage: catalog.applyMessage,
    updateSharedUi: (): Promise<void> => Promise.resolve(),
  });
  try {
    panorama.openPanoramaWizard();
    const first = panorama.generatePanoramaPreviews();
    const repeated = panorama.generatePanoramaPreviews();
    expect(paths).toHaveLength(1);
    expect(panorama.operation.peek()).toBe("preview");
    panorama.updatePanorama({ panoramaName: "Newer local name" });
    pending.resolve(
      Response.json({
        type: "patch",
        version: fixture.version,
        created_project_id: 7,
        panorama: { busy: false, projects: [project(7), project(99)] },
      }),
    );
    await Promise.all([first, repeated]);
    expect(paths).toEqual(["api/panoramas", "api/panoramas/7/previews"]);
    expect(panorama.state.peek().panoramaProjectId).toBe(7);
    expect(panorama.state.peek().panoramaName).toBe("Newer local name");
    expect(panorama.state.peek().panoramaMessage).toBe("");
    expect(panorama.operation.peek()).toBe("idle");
  } finally {
    globalThis.fetch = originalFetch;
    panorama[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("an old panorama creation cannot select a project in a reopened wizard", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const pending = delayedResponse();
  let requests = 0;
  globalThis.fetch = (): Promise<Response> => {
    requests += 1;
    return pending.response;
  };
  const fixture = reviewFixture();
  const catalog = new ReviewModel();
  catalog.applyMessage(fixture);
  const panorama = new PanoramaModel(catalog, {
    applyMessage: catalog.applyMessage,
    updateSharedUi: (): Promise<void> => Promise.resolve(),
  });
  try {
    panorama.openPanoramaWizard();
    const request = panorama.generatePanoramaPreviews();
    panorama.closePanoramaWizard();
    panorama.openPanoramaWizard();
    panorama.updatePanorama({ panoramaName: "Different dialog" });
    pending.resolve(
      Response.json({
        type: "patch",
        version: fixture.version,
        created_project_id: 7,
        panorama: { busy: false, projects: [project(7)] },
      }),
    );
    await request;
    expect(requests).toBe(1);
    expect(panorama.state.peek().panoramaProjectId).toBeNull();
    expect(panorama.state.peek().panoramaName).toBe("Different dialog");
    expect(catalog.getConfirmedState().data?.panorama.projects[0]?.id).toBe(7);
  } finally {
    globalThis.fetch = originalFetch;
    panorama[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("closing diffusion while its save barrier waits cannot start an obsolete preview", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const barrier = delayedBarrier();
  const entered = delayedBarrier();
  let requests = 0;
  globalThis.fetch = (): Promise<Response> => {
    requests += 1;
    return Promise.resolve(Response.json(diffusionFixture()));
  };
  const catalog = new ReviewModel();
  catalog.applyMessage(reviewFixture());
  const commit = immediateCommit(catalog, barrier.pending);
  const diffusion = new DiffusionModel(catalog, {
    applyMessage: catalog.applyMessage,
    commit: (scope, operation) => {
      entered.release();
      return commit(scope, operation);
    },
  });
  try {
    diffusion.openDiffusion();
    await entered.pending;
    expect(requests).toBe(0);
    diffusion.closeDiffusion();
    barrier.release();
    await barrier.pending;
    expect(requests).toBe(0);
    expect(diffusion.state.peek().diffusionOpen).toBe(false);
    expect(diffusion.state.peek().diffusionJob).toBeNull();
  } finally {
    barrier.release();
    globalThis.fetch = originalFetch;
    diffusion[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("superseded diffusion cannot replace newer settings when abort is ignored", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const pending = delayedResponse();
  const entered = delayedBarrier();
  let requests = 0;
  globalThis.fetch = (): Promise<Response> => {
    requests += 1;
    if (requests === 1) {
      entered.release();
      return pending.response;
    }
    return Promise.resolve(
      Response.json({ ...diffusionFixture(), id: 10, settings: { ...diffusionFixture().settings, softness: 25 } }),
    );
  };
  const catalog = new ReviewModel();
  catalog.applyMessage(reviewFixture());
  const diffusion = new DiffusionModel(catalog, {
    applyMessage: catalog.applyMessage,
    commit: immediateCommit(catalog),
  });
  try {
    diffusion.openDiffusion();
    await entered.pending;
    diffusion.setDiffusionSettings({ softness: 25 });
    diffusion.requestDiffusionPreview();
    await expect.poll(() => diffusion.state.peek().diffusionJob?.id).toBe(10);
    pending.resolve(Response.json(diffusionFixture()));
    await pending.response;
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(diffusion.state.peek().diffusionJob?.id).toBe(10);
    expect(diffusion.state.peek().diffusionSettings?.softness).toBe(25);
    expect(requests).toBe(2);
  } finally {
    pending.resolve(Response.json(diffusionFixture()));
    globalThis.fetch = originalFetch;
    diffusion[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("diffusion save owns one flight and commits the requested inheritance scope", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const pending = delayedResponse();
  const scopes: Parameters<ToolSessionActions["commit"]>[0][] = [];
  const paths: string[] = [];
  globalThis.fetch = (input): Promise<Response> => {
    paths.push(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
    return pending.response;
  };
  const fixture = reviewFixture();
  const catalog = new ReviewModel();
  catalog.applyMessage(fixture);
  const commit = immediateCommit(catalog);
  const diffusion = new DiffusionModel(catalog, {
    applyMessage: catalog.applyMessage,
    commit: (scope, operation) => {
      scopes.push(scope);
      return commit(scope, operation);
    },
  });
  try {
    diffusion.openDiffusion();
    const first = diffusion.applyDiffusion("all");
    const repeated = diffusion.applyDiffusion("current");
    diffusion.closeDiffusion();
    expect(diffusion.state.peek().diffusionOpen).toBe(true);
    expect(diffusion.state.peek().diffusionSaving).toBe(true);
    await expect.poll(() => paths.length).toBe(1);
    expect(scopes).toEqual(["all"]);
    expect(paths).toEqual(["api/diffusion/settings"]);
    pending.resolve(Response.json({ type: "patch", version: fixture.version }));
    await Promise.all([first, repeated]);
    expect(diffusion.state.peek().diffusionOpen).toBe(false);
    expect(diffusion.state.peek().diffusionSaving).toBe(false);
  } finally {
    pending.resolve(Response.json({ type: "patch", version: fixture.version }));
    globalThis.fetch = originalFetch;
    diffusion[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});

test("closed or disposed information dialogs reject queued detail actions", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  let requests = 0;
  globalThis.fetch = (): Promise<Response> => {
    requests += 1;
    return Promise.resolve(new Response("[Exposure]"));
  };
  const fixture = reviewFixture();
  const catalog = new ReviewModel();
  catalog.applyMessage(fixture);
  const information = new InformationModel(catalog);
  const profile = required(fixture.profiles[0]);
  const image = required(fixture.images[0]);
  try {
    information.openProfileInfo(profile);
    information.closeProfileInfo();
    await information.loadProfilePp3(image, profile);
    expect(requests).toBe(0);
    information[Symbol.dispose]();
    information.openProfileInfo(profile);
    information.openCommandInvocation();
    await information.loadProfilePp3(image, profile);
    expect(information.state.peek().profileInfoProfileIndex).toBeNull();
    expect(information.state.peek().commandInvocationOpen).toBe(false);
    expect(requests).toBe(0);
  } finally {
    globalThis.fetch = originalFetch;
    information[Symbol.dispose]();
    catalog[Symbol.dispose]();
  }
});
