/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */
import type { RequestContracts } from "./requests";
import type { ResponseContracts } from "./responses";
import * as validators from "./validators.mjs";
/** Endpoint bindings originate in Rust, preventing caller-selected response assertions. */
export interface OperationContracts {
  state: { request: undefined; response: ResponseContracts["state"] };
  review: { request: RequestContracts["review"]; response: ResponseContracts["patch"] };
  ui: { request: RequestContracts["ui"]; response: ResponseContracts["patch"] };
  burst: { request: RequestContracts["burst"]; response: ResponseContracts["patch"] };
  publish: { request: RequestContracts["publish"]; response: ResponseContracts["patch"] };
  sampler_create: { request: RequestContracts["sampler_create"]; response: ResponseContracts["sampler_job"] };
  sampler_get: { request: undefined; response: ResponseContracts["sampler_job"] };
  sampler_priority: { request: RequestContracts["sampler_priority"]; response: ResponseContracts["sampler_job"] };
  sampler_select: { request: RequestContracts["sampler_select"]; response: ResponseContracts["sampler_job"] };
  diffusion_create: { request: RequestContracts["diffusion_create"]; response: ResponseContracts["diffusion_job"] };
  diffusion_get: { request: undefined; response: ResponseContracts["diffusion_job"] };
  diffusion_apply: { request: RequestContracts["diffusion_apply"]; response: ResponseContracts["patch"] };
  diffusion_reset: { request: RequestContracts["diffusion_reset"]; response: ResponseContracts["patch"] };
  panorama_create: { request: RequestContracts["panorama_create"]; response: ResponseContracts["patch"] };
  panorama_update: { request: RequestContracts["panorama_update"]; response: ResponseContracts["patch"] };
  panorama_previews: { request: RequestContracts["panorama_previews"]; response: ResponseContracts["patch"] };
  panorama_render: { request: RequestContracts["panorama_render"]; response: ResponseContracts["patch"] };
  events: { request: undefined; response: ResponseContracts["message"] };
}
/** Concrete decoders and route templates for every supported JSON operation. */
export const operations = {
  state: {
    method: "GET",
    path: "api/state",
    allowEmptyRequest: false,
    decode: validators.validateResponseState,
    hasRequest: false,
  },
  review: {
    method: "POST",
    path: "api/review",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  ui: {
    method: "POST",
    path: "api/ui",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  burst: {
    method: "PATCH",
    path: "api/bursts/{burst_id}",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  publish: {
    method: "POST",
    path: "api/publish",
    allowEmptyRequest: true,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  sampler_create: {
    method: "POST",
    path: "api/sampler/jobs",
    allowEmptyRequest: false,
    decode: validators.validateResponseSamplerJob,
    hasRequest: true,
  },
  sampler_get: {
    method: "GET",
    path: "api/sampler/jobs/{job_id}",
    allowEmptyRequest: false,
    decode: validators.validateResponseSamplerJob,
    hasRequest: false,
  },
  sampler_priority: {
    method: "POST",
    path: "api/sampler/jobs/{job_id}/priority",
    allowEmptyRequest: false,
    decode: validators.validateResponseSamplerJob,
    hasRequest: true,
  },
  sampler_select: {
    method: "POST",
    path: "api/sampler/jobs/{job_id}/profiles/{entry_key}",
    allowEmptyRequest: false,
    decode: validators.validateResponseSamplerJob,
    hasRequest: true,
  },
  diffusion_create: {
    method: "POST",
    path: "api/diffusion/jobs",
    allowEmptyRequest: false,
    decode: validators.validateResponseDiffusionJob,
    hasRequest: true,
  },
  diffusion_get: {
    method: "GET",
    path: "api/diffusion/jobs/{job_id}",
    allowEmptyRequest: false,
    decode: validators.validateResponseDiffusionJob,
    hasRequest: false,
  },
  diffusion_apply: {
    method: "POST",
    path: "api/diffusion/settings",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  diffusion_reset: {
    method: "DELETE",
    path: "api/diffusion/settings",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  panorama_create: {
    method: "POST",
    path: "api/panoramas",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  panorama_update: {
    method: "PATCH",
    path: "api/panoramas/{project_id}",
    allowEmptyRequest: false,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  panorama_previews: {
    method: "POST",
    path: "api/panoramas/{project_id}/previews",
    allowEmptyRequest: true,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  panorama_render: {
    method: "POST",
    path: "api/panoramas/{project_id}/render",
    allowEmptyRequest: true,
    decode: validators.validateResponsePatch,
    hasRequest: true,
  },
  events: {
    method: "GET",
    path: "api/events",
    allowEmptyRequest: false,
    decode: validators.validateResponseMessage,
    hasRequest: false,
  },
} as const;
