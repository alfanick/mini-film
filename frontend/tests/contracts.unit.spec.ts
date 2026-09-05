/** Exercise generated Rust boundaries against representative fixtures and malformed data, independently of browsers. */
import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import {
  validateRequestReview,
  validateRequestUi,
  validateResponseDiffusionJob,
  validateResponseError,
  validateResponseKeepalive,
  validateResponseMessage,
  validateResponsePatch,
  validateResponseSamplerJob,
  validateResponseState,
} from "../review/generated/validators.mjs";
import { decodeKeepalive, decodeStateMessage, reviewApi } from "../review/core/api";
import { reviewFixture, diffusionFixture, samplerFixture } from "./fixtures";
import type { OperationContracts } from "../review/generated/operations";
import { operations } from "../review/generated/operations";
import { validateOutgoingRequest } from "./harness";

/** Spell each route and accepted body independently so contract changes cannot silently weaken the harness. */
const outgoingFixtures = {
  state: { method: "GET", path: "state", body: undefined },
  review: { method: "POST", path: "review", body: { image_id: 1, rating: 2, tags: ["007", "007"] } },
  ui: { method: "POST", path: "ui", body: { current_image_id: null, labels: ["red", "red"] } },
  burst: { method: "PATCH", path: "bursts/burst-1", body: { expanded: true } },
  publish: { method: "POST", path: "publish", body: {} },
  sampler_create: { method: "POST", path: "sampler/jobs", body: { image_id: 1 } },
  sampler_get: { method: "GET", path: "sampler/jobs/7", body: undefined },
  sampler_priority: { method: "POST", path: "sampler/jobs/7/priority", body: {} },
  sampler_select: {
    method: "POST",
    path: "sampler/jobs/7/profiles/profile%2Fa%20%26%20b",
    body: { enabled: true, scope: "current" },
  },
  diffusion_create: { method: "POST", path: "diffusion/jobs", body: { image_id: 1, profile_index: 0, settings: {} } },
  diffusion_get: { method: "GET", path: "diffusion/jobs/7", body: undefined },
  diffusion_apply: {
    method: "POST",
    path: "diffusion/settings",
    body: { image_id: 1, profile_index: 0, scope: "all", settings: {} },
  },
  diffusion_reset: {
    method: "DELETE",
    path: "diffusion/settings",
    body: { image_id: 1, profile_index: 0, scope: "current" },
  },
  panorama_create: { method: "POST", path: "panoramas", body: { image_ids: [2, 1] } },
  panorama_update: { method: "PATCH", path: "panoramas/7", body: { name: null, selected_projection: null } },
  panorama_previews: { method: "POST", path: "panoramas/7/previews", body: {} },
  panorama_render: { method: "POST", path: "panoramas/7/render", body: {} },
  events: { method: "GET", path: "events", body: undefined },
} satisfies {
  [Name in keyof OperationContracts]: {
    method: string;
    path: string;
    body: OperationContracts[Name]["request"];
  };
};

/** Narrow a fixture catalog without asserting anything about the untrusted JSON values inside it. */
function isCatalog(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

test("actual Rust-serialized fixtures satisfy the generated response validators", (): void => {
  const fixtures: unknown = JSON.parse(readFileSync("frontend/review/generated/fixtures.json", "utf8"));
  if (!isCatalog(fixtures)) throw new Error("Rust fixture catalog must be an object");
  const validators: Readonly<Record<string, (value: unknown) => boolean>> = {
    state: validateResponseState,
    patch: validateResponsePatch,
    sampler_job: validateResponseSamplerJob,
    diffusion_job: validateResponseDiffusionJob,
    keepalive: validateResponseKeepalive,
    error: validateResponseError,
  };
  expect(Object.keys(fixtures).sort()).toEqual(Object.keys(validators).sort());
  for (const [name, validate] of Object.entries(validators)) expect(validate(fixtures[name]), name).toBe(true);
});

test("browser fixtures match actual contracts and incompatible payloads never decode", (): void => {
  expect(validateResponseState(reviewFixture())).toBe(true);
  expect(validateResponseSamplerJob(samplerFixture())).toBe(true);
  expect(validateResponseDiffusionJob(diffusionFixture())).toBe(true);
  expect(() => decodeStateMessage("not JSON")).toThrow("malformed JSON");
  expect(() => decodeStateMessage(JSON.stringify({ ...reviewFixture(), images: "wrong" }))).toThrow("incompatible");
  expect(() => decodeKeepalive('{"version":"23.0.0"}')).toThrow("incompatible");
  expect(validateResponseMessage({ type: "patch", version: "23.0.0", client_count: 4 })).toBe(true);
});

test("contracts distinguish omitted fields from nullable clears without changing request compatibility", (): void => {
  expect(validateResponsePatch({ type: "patch", version: "23.0.0" })).toBe(true);
  expect(validateResponsePatch({ type: "patch", version: "23.0.0", invocation: null })).toBe(true);
  expect(validateResponsePatch({ type: "patch", version: "23.0.0", images: null })).toBe(false);
  expect(validateResponseState({ ...reviewFixture(), invocation: undefined })).toBe(false);
  expect(validateRequestReview({ image_id: 1, rating: 200, tags: [], unknown_future_field: true })).toBe(true);
  expect(validateRequestReview({ image_id: 1, rating: 256, tags: [] })).toBe(false);
  expect(validateRequestUi({ labels: ["red", "red"] })).toBe(true);
});

test("outgoing browser requests cover every named Rust operation with the correct method and path", (): void => {
  expect(Object.keys(outgoingFixtures).sort()).toEqual(Object.keys(operations).sort());
  for (const [name, request] of Object.entries(outgoingFixtures)) {
    expect(validateOutgoingRequest(request.path, request.method, request.body)).toBe(name);
    expect(() => validateOutgoingRequest(request.path, "OPTIONS", request.body), name).toThrow("Rust JSON operation");
    expect(() => validateOutgoingRequest(`${request.path}/unexpected`, request.method, request.body), name).toThrow(
      "Rust JSON operation",
    );
  }
  expect(() => validateOutgoingRequest("sampler/jobs/7/profiles/profile/a", "POST", {})).toThrow("Rust JSON operation");
  expect(() => validateOutgoingRequest("panoramas//render", "POST", {})).toThrow("Rust JSON operation");
});

test("outgoing bodies obey Rust empty-body rules without treating explicit null as absent", (): void => {
  const emptyBodies = new Set(["publish", "panorama_previews", "panorama_render"]);
  for (const [name, request] of Object.entries(outgoingFixtures)) {
    expect(() => validateOutgoingRequest(request.path, request.method, null), name).toThrow();
    if (request.body === undefined || emptyBodies.has(name))
      expect(validateOutgoingRequest(request.path, request.method, undefined)).toBe(name);
    else
      expect(() => validateOutgoingRequest(request.path, request.method, undefined), name).toThrow(
        "Rust JSON contract",
      );
    if (request.body === undefined)
      expect(() => validateOutgoingRequest(request.path, request.method, {}), name).toThrow("must not contain");
  }
});

test("outgoing validation rejects nested type errors but retains Rust defaults and compatibility fields", (): void => {
  expect(() =>
    validateOutgoingRequest("review", "POST", {
      image_id: 1,
      rating: 2,
      tags: [],
      retouch: { adjustments: { exposure: "bright" } },
    }),
  ).toThrow("Rust JSON contract");
  expect(() =>
    validateOutgoingRequest("diffusion/settings", "DELETE", { image_id: 1, profile_index: 0, scope: "invalid" }),
  ).toThrow("Rust JSON contract");
  expect(
    validateOutgoingRequest("review", "POST", {
      image_id: 1,
      rating: 2,
      tags: ["007", "007"],
      label: "red",
      labels: ["red", "red"],
      selected_profile_index: null,
      enabled_profile_indexes: null,
      publish_profile_indexes: [0],
      unknown_future_field: true,
    }),
  ).toBe("review");
});

test("route identities are encoded and successful empty JSON responses are rejected", async (): Promise<void> => {
  const originalFetch = globalThis.fetch;
  const requests: string[] = [];
  globalThis.fetch = (input: string | URL | Request): Promise<Response> => {
    requests.push(input instanceof Request ? input.url : String(input));
    return Promise.resolve(new Response(null, { status: 204 }));
  };
  try {
    await expect(
      reviewApi.sampler_select({
        params: { job_id: 7, entry_key: "profile/a & b" },
        body: { enabled: true, scope: "current" },
      }),
    ).rejects.toThrow("malformed JSON");
    expect(requests).toEqual(["api/sampler/jobs/7/profiles/profile%2Fa%20%26%20b"]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
