/**
 * Keep HTTP paths and JSON transport independent of components.
 * Relative URLs preserve review sessions served beneath a reverse-proxy prefix.
 */

export type HttpMethod = "GET" | "POST" | "PATCH" | "DELETE";

/** Resolve an API/media path relative to the current review page. */
export function reviewUrl(path: string): string {
  return path.replace(/^\/+/, "");
}

/** Request a typed same-application response; successful empty deletes return null. */
export async function requestJson<T>(
  path: string,
  method: HttpMethod = "GET",
  body?: unknown,
  signal?: AbortSignal,
): Promise<T | null> {
  const response = await fetch(reviewUrl(path), {
    method,
    cache: "no-store",
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
  });
  const text = await response.text();
  if (!response.ok) {
    let message = `${path} ${response.status}`;
    try {
      const data: unknown = JSON.parse(text);
      if (typeof data === "object" && data !== null && "error" in data && typeof data.error === "string") {
        message = data.error;
      }
    } catch {
      // A non-JSON server error still carries the HTTP status above.
    }
    throw new Error(message);
  }
  // This is the single typed boundary to the Rust API. DTOs mirror its serialized schema.
  return text ? (JSON.parse(text) as T) : null;
}

/** Format unknown thrown values without weakening application types. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Identify cancelled browser operations so closing a tool does not display an error. */
export function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}
