/** Verify neutral sampling, scoped edit barriers and generation ownership independently of view lifetimes. */
import { expect, test } from "@playwright/test";
import { ReviewModel } from "../review/core/model";
import { SamplerModel } from "../review/features/sampler/model";
import type { CommitScope } from "../review/session/barriers";
import type { ToolSessionActions } from "../review/tools/types";
import type { SamplerJob } from "../review/core/types";
import { reviewFixture, samplerFixture } from "./fixtures";
import { required } from "./required";

/** Resolve delayed reads or mutations only after the test establishes a newer model state. */
function deferredResponse(): { response: Promise<Response>; complete: (response: Response) => void } {
  let complete: (response: Response) => void = (): void => {
    throw new Error("Response was not initialized");
  };
  const response = new Promise<Response>((resolve): void => {
    complete = resolve;
  });
  return { response, complete };
}

/** Compose the real model with an observable barrier port and no hidden provider or DOM dependencies. */
function samplerHarness(): {
  readonly catalog: InstanceType<typeof ReviewModel>;
  readonly sampler: InstanceType<typeof SamplerModel>;
  readonly scopes: CommitScope[];
} {
  const catalog = new ReviewModel();
  catalog.applyMessage(reviewFixture());
  const scopes: CommitScope[] = [];
  const session: ToolSessionActions = {
    applyMessage: catalog.applyMessage,
    updateSharedUi: (): Promise<void> => Promise.resolve(),
    commit: async (scope, operation) => {
      scopes.push(scope);
      return operation({
        snapshot: catalog.getConfirmedState(),
        affectedOutputs: new Set(),
        refresh: (): Promise<ReturnType<typeof catalog.getConfirmedState>> =>
          Promise.resolve(catalog.getConfirmedState()),
      });
    },
  };
  return { catalog, sampler: new SamplerModel(catalog, session), scopes };
}

test("neutral sampling skips edits while enabling profiles commits its requested scope", async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (): Promise<Response> => Promise.resolve(Response.json(samplerFixture()));
  const { catalog, sampler, scopes } = samplerHarness();
  try {
    await sampler.openSampler();
    expect(scopes).toEqual([]);
    const entry = required(sampler.state.peek().samplerJob?.entries[0]);
    await sampler.updateSamplerSelection(entry, "current", true);
    await sampler.updateSamplerSelection(entry, "all", true);
    await sampler.updateSamplerSelection(entry, "current", false);
    expect(scopes).toEqual([[1], "all"]);
    expect(sampler.state.peek().samplerPendingSelections.size).toBe(0);
  } finally {
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("sampler open is single-flight and a closed dialog rejects even an unabortable late response", async () => {
  const originalFetch = globalThis.fetch;
  let complete: (response: Response) => void = (): void => {
    throw new Error("Response was not initialized");
  };
  const response = new Promise<Response>((resolve): void => {
    complete = resolve;
  });
  let requests = 0;
  globalThis.fetch = (): Promise<Response> => {
    requests += 1;
    return response;
  };
  const { catalog, sampler } = samplerHarness();
  try {
    const first = sampler.openSampler();
    await sampler.openSampler();
    expect(requests).toBe(1);
    sampler.closeSampler();
    complete(Response.json(samplerFixture()));
    await first;
    expect(sampler.state.peek().samplerOpen).toBe(false);
    expect(sampler.state.peek().samplerJob).toBeNull();
  } finally {
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("closing sampler never aborts an issued selection mutation or exposes it in a reopened dialog", async () => {
  const originalFetch = globalThis.fetch;
  let complete: (response: Response) => void = (): void => {
    throw new Error("Mutation was not initialized");
  };
  const response = new Promise<Response>((resolve): void => {
    complete = resolve;
  });
  let mutationSignal: AbortSignal | null | undefined;
  globalThis.fetch = (input, init): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.includes("/profiles/")) {
      mutationSignal = init?.signal;
      return response;
    }
    return Promise.resolve(Response.json(samplerFixture()));
  };
  const { catalog, sampler } = samplerHarness();
  try {
    await sampler.openSampler();
    const entry = required(sampler.state.peek().samplerJob?.entries[0]);
    const saving = sampler.updateSamplerSelection(entry, "current", true);
    sampler.closeSampler();
    await sampler.openSampler();
    const next = samplerFixture();
    next.id = 99;
    complete(Response.json(next));
    await saving;
    expect(mutationSignal?.aborted ?? false).toBe(false);
    expect(sampler.state.peek().samplerJob?.id).toBe(samplerFixture().id);
  } finally {
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("equal sampler priority inputs survive unrelated completed-job updates", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  let complete: (response: Response) => void = (): void => {
    throw new Error("Priority response was not initialized");
  };
  const response = new Promise<Response>((resolve): void => {
    complete = resolve;
  });
  const signals: AbortSignal[] = [];
  globalThis.fetch = (input, init): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.endsWith("/priority")) {
      signals.push(required(init?.signal));
      return response;
    }
    return Promise.resolve(Response.json(samplerFixture()));
  };
  const { catalog, sampler } = samplerHarness();
  try {
    await sampler.openSampler();
    sampler.setVisibleEntries(["classic"]);
    await expect.poll(() => signals.length).toBe(1);
    const entry = required(sampler.state.peek().samplerJob?.entries[0]);
    await sampler.updateSamplerSelection(entry, "current", false);
    expect(required(signals[0]).aborted).toBe(false);
    expect(signals).toHaveLength(1);
    complete(Response.json(samplerFixture()));
  } finally {
    complete(Response.json(samplerFixture()));
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("returning to a canceled priority signature submits the current viewport again", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const signals: AbortSignal[] = [];
  const completions: ((response: Response) => void)[] = [];
  globalThis.fetch = (input, init): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.endsWith("/priority")) {
      signals.push(required(init?.signal));
      return new Promise<Response>((resolve): void => {
        completions.push(resolve);
      });
    }
    return Promise.resolve(Response.json(samplerFixture()));
  };
  const { catalog, sampler } = samplerHarness();
  try {
    await sampler.openSampler();
    sampler.setVisibleEntries(["classic"]);
    await expect.poll(() => signals.length).toBe(1);
    sampler.setVisibleEntries([]);
    sampler.setVisibleEntries(["classic"]);
    expect(required(signals[0]).aborted).toBe(true);
    await expect.poll(() => signals.length).toBe(2);
    expect(required(signals[1]).aborted).toBe(false);
    sampler.closeSampler();
    expect(required(signals[1]).aborted).toBe(true);
  } finally {
    for (const complete of completions) complete(Response.json(samplerFixture()));
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("sampler serializes selection mutations but captures enabling barriers immediately", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const older = deferredResponse();
  const mutations: string[] = [];
  globalThis.fetch = (input): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (url.includes("/profiles/")) {
      mutations.push(url);
      return mutations.length === 1 ? older.response : Promise.resolve(Response.json(samplerFixture()));
    }
    return Promise.resolve(Response.json(samplerFixture()));
  };
  const { catalog, sampler, scopes } = samplerHarness();
  try {
    await sampler.openSampler();
    const entry = required(sampler.state.peek().samplerJob?.entries[0]);
    const first = sampler.updateSamplerSelection(entry, "current", false);
    const second = sampler.updateSamplerSelection(entry, "all", true);
    expect(scopes).toEqual(["all"]);
    await expect.poll(() => mutations.length).toBe(1);
    older.complete(Response.json(samplerFixture()));
    await Promise.all([first, second]);
    expect(mutations).toHaveLength(2);
    expect(sampler.state.peek().samplerPendingSelections.size).toBe(0);
  } finally {
    older.complete(Response.json(samplerFixture()));
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("a sampler poll started before selection cannot overwrite its acknowledged result", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const poll = deferredResponse();
  const processing: SamplerJob = { ...samplerFixture(), status: "rendering" };
  const selected: SamplerJob = {
    ...processing,
    entries: processing.entries.map((entry) => ({ ...entry, current_enabled: false, selected: false })),
  };
  let readStarted = false;
  globalThis.fetch = (input, init): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (init?.method === "GET" && url.endsWith("/7")) {
      readStarted = true;
      return poll.response;
    }
    if (url.includes("/profiles/")) return Promise.resolve(Response.json(selected));
    return Promise.resolve(Response.json(processing));
  };
  const { catalog, sampler } = samplerHarness();
  try {
    await sampler.openSampler();
    await expect.poll(() => readStarted).toBe(true);
    const entry = required(sampler.state.peek().samplerJob?.entries[0]);
    await sampler.updateSamplerSelection(entry, "current", false);
    expect(sampler.state.peek().samplerJob?.entries[0]?.current_enabled).toBe(false);
    poll.complete(Response.json(processing));
    await poll.response;
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(sampler.state.peek().samplerJob?.entries[0]?.current_enabled).toBe(false);
  } finally {
    poll.complete(Response.json(processing));
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});

test("closing sampler aborts its pending read and rejects an abort-insensitive response", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const poll = deferredResponse();
  const processing: SamplerJob = { ...samplerFixture(), status: "rendering" };
  let readSignal: AbortSignal | null | undefined;
  globalThis.fetch = (input, init): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (init?.method === "GET" && url.endsWith("/7")) {
      readSignal = init.signal;
      return poll.response;
    }
    return Promise.resolve(Response.json(processing));
  };
  const { catalog, sampler } = samplerHarness();
  try {
    await sampler.openSampler();
    await expect.poll(() => Boolean(readSignal)).toBe(true);
    sampler.closeSampler();
    expect(required(readSignal).aborted).toBe(true);
    poll.complete(Response.json(processing));
    await poll.response;
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(sampler.state.peek().samplerOpen).toBe(false);
    expect(sampler.state.peek().samplerJob).toBeNull();
  } finally {
    poll.complete(Response.json(processing));
    sampler[Symbol.dispose]();
    catalog[Symbol.dispose]();
    globalThis.fetch = originalFetch;
  }
});
