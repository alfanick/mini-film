/** Retain failed semantic assignments for explicit recovery without replaying stale or non-idempotent actions. */
import { signal, type ReadonlySignal } from "@preact/signals";
import type { ReviewIntent } from "./commands";

/** Local generations order browser intentions only; they are not server revisions. */
export interface IntentTicket {
  imageId: number;
  key: string;
  generation: number;
  intent: ReviewIntent;
}

/** A lost response leaves commitment uncertain; rating-and-advance can only be checked, never retried here. */
export interface IntentFailure extends IntentTicket {
  message: string;
  retryable: boolean;
}

/** Expose readonly failures and generation-checked transitions for deterministic tests and session recovery. */
export interface FailureTracker {
  failures: ReadonlySignal<readonly IntentFailure[]>;
  begin: (imageId: number, intent: ReviewIntent) => IntentTicket;
  fail: (ticket: IntentTicket, message: string) => void;
  clear: (ticket: IntentTicket) => void;
  current: (ticket: IntentTicket) => boolean;
}

/** Coupled profile operations share ownership; unrelated labels, ratings and per-profile filters do not. */
function domain(intent: ReviewIntent): string {
  switch (intent.kind) {
    case "with-draft":
      return domain(intent.intent);
    case "fields":
      return intent.fields.rating !== undefined ? "rating" : "review";
    case "label":
      return "labels";
    case "bw-filter":
      return `bw:${intent.profileIndex}`;
    case "profile-enabled":
    case "profile-selected":
    case "profile-solo":
      return "profiles";
  }
}

/** Only a newer command in the same image/domain supersedes a retained failure. */
export function createFailureTracker(): FailureTracker {
  const failures = signal<readonly IntentFailure[]>([]);
  const latest = new Map<string, number>();
  let generation = 0;
  /** Reject stale completions even when a test transport resolves promises out of order. */
  const current = (ticket: IntentTicket): boolean => latest.get(ticket.key) === ticket.generation;
  /** Remove only the failure owned by this still-current local intention. */
  const clear = (ticket: IntentTicket): void => {
    if (current(ticket)) failures.value = failures.peek().filter((failure) => failure.key !== ticket.key);
  };
  return {
    failures,
    current,
    clear,
    begin(imageId, intent): IntentTicket {
      while (intent.kind === "with-draft") intent = intent.intent;
      const ticket = { imageId, intent, key: `${imageId}:${domain(intent)}`, generation: ++generation };
      latest.set(ticket.key, ticket.generation);
      clear(ticket);
      return ticket;
    },
    fail(ticket, message): void {
      if (!current(ticket)) return;
      failures.value = [
        ...failures.peek().filter((failure) => failure.key !== ticket.key),
        {
          ...ticket,
          message,
          retryable: !(ticket.intent.kind === "fields" && ticket.intent.fields.advance_after_update),
        },
      ];
    },
  };
}
