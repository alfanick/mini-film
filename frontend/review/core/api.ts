/** Expose Rust-bound HTTP operations and validated SSE decoders without caller-selected response assertions. */
import { decodeJson } from "./transport";
import { validateResponseKeepalive, validateResponseMessage } from "../generated/validators.mjs";
import type { ReviewKeepalive, ReviewStateMessage } from "../generated/responses";

export { errorMessage, isAbortError, reviewUrl } from "./transport";
export { reviewApi } from "../generated/client";

/** Decode full snapshots and tagged patches before reconciliation touches existing state. */
export function decodeStateMessage(text: string): ReviewStateMessage {
  return decodeJson(text, validateResponseMessage, "Live review update");
}

/** Validate the distinct named keepalive envelope before version/reconnection handling. */
export function decodeKeepalive(text: string): ReviewKeepalive {
  return decodeJson(text, validateResponseKeepalive, "Live review keepalive");
}
