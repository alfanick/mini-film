/** Validate every JSON boundary before exposing data to Preact, with endpoints bound by generated Rust contracts. */

/** Request capabilities shared by generated operations without allowing callers to select a response type. */
export interface OperationSpec<Response> {
  readonly method: "GET" | "POST" | "PATCH" | "DELETE";
  readonly path: string;
  readonly allowEmptyRequest: boolean;
  readonly decode: (value: unknown) => value is Response;
  readonly hasRequest: boolean;
}

/** Require bodies and path identities exactly where the Rust operation manifest declares them. */
export type OperationOptions<Request, Parameters> = (undefined extends Request
  ? { body?: Request }
  : { body: Request }) &
  (keyof Parameters extends never ? { params?: Parameters } : { params: Parameters }) & { signal?: AbortSignal };

/** Resolve API/media paths relative to the page so reverse-proxy prefixes remain supported. */
export function reviewUrl(path: string): string {
  return path.replace(/^\/+/, "");
}

/** Normalize thrown values for visible error states without weakening application types. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Distinguish deliberate request cancellation from actionable failures. */
export function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

/** Parse untrusted JSON with a useful context; no parsed value escapes without a decoder. */
export function decodeJson<T>(text: string, decode: (value: unknown) => value is T, context: string): T {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error(`${context}: the server returned malformed JSON`);
  }
  if (!decode(value)) throw new Error(`${context}: the server returned an incompatible response`);
  return value;
}

/** Bind validated transport once to an operation's real request, response, and path-parameter contracts. */
export function createOperation<Request, Response, Parameters extends Readonly<Record<string, string | number>>>(
  spec: OperationSpec<Response>,
): (options: OperationOptions<Request, Parameters>) => Promise<Response> {
  return async (options: OperationOptions<Request, Parameters>): Promise<Response> => {
    const body = options.body;
    if (body === undefined ? spec.hasRequest && !spec.allowEmptyRequest : !spec.hasRequest) {
      throw new Error(`${spec.path}: invalid request`);
    }
    const parameters: Readonly<Record<string, string | number>> = options.params ?? {};
    const path = spec.path.replace(/\{([a-z_]+)\}/g, (_match: string, key: string): string => {
      const value = parameters[key];
      if (value === undefined) throw new Error(`${spec.path}: missing ${key}`);
      return encodeURIComponent(String(value));
    });
    const response = await fetch(reviewUrl(path), {
      method: spec.method,
      cache: "no-store",
      ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
      ...(options.signal === undefined ? {} : { signal: options.signal }),
    });
    const text = await response.text();
    if (!response.ok) {
      let message = `${path} ${response.status}`;
      try {
        const error: unknown = JSON.parse(text);
        if (typeof error === "object" && error !== null && "error" in error && typeof error.error === "string") {
          message = error.error;
        }
      } catch {
        // HTML/plain-text failures still carry a useful HTTP status.
      }
      throw new Error(message);
    }
    // Successful JSON endpoints always return JSON, including DELETE settings.
    return decodeJson(text, spec.decode, path);
  };
}
