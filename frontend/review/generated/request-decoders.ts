/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */
import type { OperationContracts } from "./operations";
import { operations } from "./operations";
import * as validators from "./validators.mjs";
/** Correlate operation identity with its exact body, including deliberately empty creation requests. */
export type OperationRequest<Name extends keyof OperationContracts> =
  | OperationContracts[Name]["request"]
  | ((typeof operations)[Name]["allowEmptyRequest"] extends true ? undefined : never);
/** Test-only decoders verify the outgoing body without adding request validators to the browser entry point. */
export const requestDecoders: {
  [Name in keyof OperationContracts]: (value: unknown) => OperationRequest<Name>;
} = {
  /** Decode the state body using its Rust-owned request contract. */
  state: (value: unknown): OperationRequest<"state"> => {
    if (value === undefined) return undefined;
    throw new Error("Invalid state request body");
  },
  /** Decode the review body using its Rust-owned request contract. */
  review: (value: unknown): OperationRequest<"review"> => {
    if (validators.validateRequestReview(value)) return value;
    throw new Error("Invalid review request body");
  },
  /** Decode the ui body using its Rust-owned request contract. */
  ui: (value: unknown): OperationRequest<"ui"> => {
    if (validators.validateRequestUi(value)) return value;
    throw new Error("Invalid ui request body");
  },
  /** Decode the burst body using its Rust-owned request contract. */
  burst: (value: unknown): OperationRequest<"burst"> => {
    if (validators.validateRequestBurst(value)) return value;
    throw new Error("Invalid burst request body");
  },
  /** Decode the publish body using its Rust-owned request contract. */
  publish: (value: unknown): OperationRequest<"publish"> => {
    if (value === undefined) return undefined;
    if (validators.validateRequestPublish(value)) return value;
    throw new Error("Invalid publish request body");
  },
  /** Decode the sampler_create body using its Rust-owned request contract. */
  sampler_create: (value: unknown): OperationRequest<"sampler_create"> => {
    if (validators.validateRequestSamplerCreate(value)) return value;
    throw new Error("Invalid sampler_create request body");
  },
  /** Decode the sampler_get body using its Rust-owned request contract. */
  sampler_get: (value: unknown): OperationRequest<"sampler_get"> => {
    if (value === undefined) return undefined;
    throw new Error("Invalid sampler_get request body");
  },
  /** Decode the sampler_priority body using its Rust-owned request contract. */
  sampler_priority: (value: unknown): OperationRequest<"sampler_priority"> => {
    if (validators.validateRequestSamplerPriority(value)) return value;
    throw new Error("Invalid sampler_priority request body");
  },
  /** Decode the sampler_select body using its Rust-owned request contract. */
  sampler_select: (value: unknown): OperationRequest<"sampler_select"> => {
    if (validators.validateRequestSamplerSelect(value)) return value;
    throw new Error("Invalid sampler_select request body");
  },
  /** Decode the diffusion_create body using its Rust-owned request contract. */
  diffusion_create: (value: unknown): OperationRequest<"diffusion_create"> => {
    if (validators.validateRequestDiffusionCreate(value)) return value;
    throw new Error("Invalid diffusion_create request body");
  },
  /** Decode the diffusion_get body using its Rust-owned request contract. */
  diffusion_get: (value: unknown): OperationRequest<"diffusion_get"> => {
    if (value === undefined) return undefined;
    throw new Error("Invalid diffusion_get request body");
  },
  /** Decode the diffusion_apply body using its Rust-owned request contract. */
  diffusion_apply: (value: unknown): OperationRequest<"diffusion_apply"> => {
    if (validators.validateRequestDiffusionApply(value)) return value;
    throw new Error("Invalid diffusion_apply request body");
  },
  /** Decode the diffusion_reset body using its Rust-owned request contract. */
  diffusion_reset: (value: unknown): OperationRequest<"diffusion_reset"> => {
    if (validators.validateRequestDiffusionReset(value)) return value;
    throw new Error("Invalid diffusion_reset request body");
  },
  /** Decode the panorama_create body using its Rust-owned request contract. */
  panorama_create: (value: unknown): OperationRequest<"panorama_create"> => {
    if (validators.validateRequestPanoramaCreate(value)) return value;
    throw new Error("Invalid panorama_create request body");
  },
  /** Decode the panorama_update body using its Rust-owned request contract. */
  panorama_update: (value: unknown): OperationRequest<"panorama_update"> => {
    if (validators.validateRequestPanoramaUpdate(value)) return value;
    throw new Error("Invalid panorama_update request body");
  },
  /** Decode the panorama_previews body using its Rust-owned request contract. */
  panorama_previews: (value: unknown): OperationRequest<"panorama_previews"> => {
    if (value === undefined) return undefined;
    if (validators.validateRequestPanoramaPreviews(value)) return value;
    throw new Error("Invalid panorama_previews request body");
  },
  /** Decode the panorama_render body using its Rust-owned request contract. */
  panorama_render: (value: unknown): OperationRequest<"panorama_render"> => {
    if (value === undefined) return undefined;
    if (validators.validateRequestPanoramaRender(value)) return value;
    throw new Error("Invalid panorama_render request body");
  },
  /** Decode the events body using its Rust-owned request contract. */
  events: (value: unknown): OperationRequest<"events"> => {
    if (value === undefined) return undefined;
    throw new Error("Invalid events request body");
  },
};
/** Preserve the operation name and decoded body as one discriminated recording. */
export type RecordedOperation = {
  [Name in keyof OperationContracts]: { name: Name; body: OperationRequest<Name> };
}[keyof OperationContracts];
/** Decode a matched HTTP route without widening the relationship between its name and body. */
export function decodeOperationRequest(name: keyof OperationContracts, body: unknown): RecordedOperation {
  switch (name) {
    case "state":
      return { name: "state", body: requestDecoders["state"](body) };
    case "review":
      return { name: "review", body: requestDecoders["review"](body) };
    case "ui":
      return { name: "ui", body: requestDecoders["ui"](body) };
    case "burst":
      return { name: "burst", body: requestDecoders["burst"](body) };
    case "publish":
      return { name: "publish", body: requestDecoders["publish"](body) };
    case "sampler_create":
      return { name: "sampler_create", body: requestDecoders["sampler_create"](body) };
    case "sampler_get":
      return { name: "sampler_get", body: requestDecoders["sampler_get"](body) };
    case "sampler_priority":
      return { name: "sampler_priority", body: requestDecoders["sampler_priority"](body) };
    case "sampler_select":
      return { name: "sampler_select", body: requestDecoders["sampler_select"](body) };
    case "diffusion_create":
      return { name: "diffusion_create", body: requestDecoders["diffusion_create"](body) };
    case "diffusion_get":
      return { name: "diffusion_get", body: requestDecoders["diffusion_get"](body) };
    case "diffusion_apply":
      return { name: "diffusion_apply", body: requestDecoders["diffusion_apply"](body) };
    case "diffusion_reset":
      return { name: "diffusion_reset", body: requestDecoders["diffusion_reset"](body) };
    case "panorama_create":
      return { name: "panorama_create", body: requestDecoders["panorama_create"](body) };
    case "panorama_update":
      return { name: "panorama_update", body: requestDecoders["panorama_update"](body) };
    case "panorama_previews":
      return { name: "panorama_previews", body: requestDecoders["panorama_previews"](body) };
    case "panorama_render":
      return { name: "panorama_render", body: requestDecoders["panorama_render"](body) };
    case "events":
      return { name: "events", body: requestDecoders["events"](body) };
  }
}
