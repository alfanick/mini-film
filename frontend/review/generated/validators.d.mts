/** Generated from Rust wire contracts; regenerate with npm run contracts:generate. */
import type { RequestContracts } from "./requests";
import type { ResponseContracts } from "./responses";
/** Validate the burst requests boundary before consuming unknown JSON. */
export function validateRequestBurst(value: unknown): value is RequestContracts["burst"];
/** Validate the diffusion_apply requests boundary before consuming unknown JSON. */
export function validateRequestDiffusionApply(value: unknown): value is RequestContracts["diffusion_apply"];
/** Validate the diffusion_create requests boundary before consuming unknown JSON. */
export function validateRequestDiffusionCreate(value: unknown): value is RequestContracts["diffusion_create"];
/** Validate the diffusion_reset requests boundary before consuming unknown JSON. */
export function validateRequestDiffusionReset(value: unknown): value is RequestContracts["diffusion_reset"];
/** Validate the panorama_create requests boundary before consuming unknown JSON. */
export function validateRequestPanoramaCreate(value: unknown): value is RequestContracts["panorama_create"];
/** Validate the panorama_previews requests boundary before consuming unknown JSON. */
export function validateRequestPanoramaPreviews(value: unknown): value is RequestContracts["panorama_previews"];
/** Validate the panorama_render requests boundary before consuming unknown JSON. */
export function validateRequestPanoramaRender(value: unknown): value is RequestContracts["panorama_render"];
/** Validate the panorama_update requests boundary before consuming unknown JSON. */
export function validateRequestPanoramaUpdate(value: unknown): value is RequestContracts["panorama_update"];
/** Validate the publish requests boundary before consuming unknown JSON. */
export function validateRequestPublish(value: unknown): value is RequestContracts["publish"];
/** Validate the review requests boundary before consuming unknown JSON. */
export function validateRequestReview(value: unknown): value is RequestContracts["review"];
/** Validate the sampler_create requests boundary before consuming unknown JSON. */
export function validateRequestSamplerCreate(value: unknown): value is RequestContracts["sampler_create"];
/** Validate the sampler_priority requests boundary before consuming unknown JSON. */
export function validateRequestSamplerPriority(value: unknown): value is RequestContracts["sampler_priority"];
/** Validate the sampler_select requests boundary before consuming unknown JSON. */
export function validateRequestSamplerSelect(value: unknown): value is RequestContracts["sampler_select"];
/** Validate the ui requests boundary before consuming unknown JSON. */
export function validateRequestUi(value: unknown): value is RequestContracts["ui"];
/** Validate the diffusion_job responses boundary before consuming unknown JSON. */
export function validateResponseDiffusionJob(value: unknown): value is ResponseContracts["diffusion_job"];
/** Validate the error responses boundary before consuming unknown JSON. */
export function validateResponseError(value: unknown): value is ResponseContracts["error"];
/** Validate the keepalive responses boundary before consuming unknown JSON. */
export function validateResponseKeepalive(value: unknown): value is ResponseContracts["keepalive"];
/** Validate the message responses boundary before consuming unknown JSON. */
export function validateResponseMessage(value: unknown): value is ResponseContracts["message"];
/** Validate the patch responses boundary before consuming unknown JSON. */
export function validateResponsePatch(value: unknown): value is ResponseContracts["patch"];
/** Validate the sampler_job responses boundary before consuming unknown JSON. */
export function validateResponseSamplerJob(value: unknown): value is ResponseContracts["sampler_job"];
/** Validate the state responses boundary before consuming unknown JSON. */
export function validateResponseState(value: unknown): value is ResponseContracts["state"];
