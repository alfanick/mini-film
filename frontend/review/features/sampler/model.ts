/** Provider-owned sampler state preserves requests through view recovery and invalidates obsolete dialog work. */
import { batch, computed, createModel, effect, signal, type ReadonlySignal } from "@preact/signals";
import { reviewApi } from "../../core/api";
import type { ReviewModelValue } from "../../core/model";
import type {
  DiffusionScope,
  SamplerEntryObservation as SamplerEntry,
  SamplerJobObservation as SamplerJob,
} from "../../core/types";
import { SAMPLER_POLL_MS, SAMPLER_PRIORITY_DEBOUNCE_MS } from "../../core/constants";
import { buildSamplerHierarchy, type SamplerSectionData } from "./helpers";
import { errorMessage, isAbortError } from "../../tools/common";
import type { ToolSessionActions } from "../../tools/types";

/** Exclusive phases prevent an old job from appearing while a new one is opening. */
type SamplerPhase =
  | { readonly status: "closed" }
  | { readonly status: "opening" }
  | { readonly status: "ready"; readonly job: SamplerJob }
  | { readonly status: "failed"; readonly error: string };

/** The overlay observes only sampler state, never unrelated catalog or connection updates. */
export interface SamplerState {
  readonly samplerOpen: boolean;
  readonly samplerLoading: boolean;
  readonly samplerError: string;
  readonly samplerJob: SamplerJob | null;
  readonly samplerExpandedSections: ReadonlySet<string>;
  readonly samplerSelectedKey: string | null;
  readonly samplerPendingSelections: ReadonlySet<string>;
}

/** Stable actions accept measurements, but never retain view-owned DOM nodes. */
export interface SamplerActions {
  openSampler(this: void): Promise<void>;
  closeSampler(this: void): void;
  selectSamplerEntry(this: void, key: string): void;
  toggleSamplerSection(this: void, key: string, expanded: boolean): void;
  updateSamplerSelection(this: void, entry: SamplerEntry, scope: DiffusionScope, enabled: boolean): Promise<void>;
  samplerSelectedEntry(this: void, job: SamplerJob | null): SamplerEntry | null;
  setVisibleEntries(this: void, keys: readonly string[]): void;
}

/** One durable model exposes readonly observations and stable commands. */
export interface SamplerModelValue extends SamplerActions {
  readonly state: ReadonlySignal<SamplerState>;
}

/** Memoized request inputs exclude changing render progress so a poll cannot cancel identical viewport priorities. */
interface SamplerPriorityInput {
  readonly jobId: number;
  readonly signature: string;
  readonly body: { visible_keys: string[]; expanded_keys: string[] };
}

/** Own neutral rendering, serial polling, priorities, and edit barriers above recoverable views. */
export const SamplerModel = createModel((catalog: ReviewModelValue, session: ToolSessionActions): SamplerModelValue => {
  const phase = signal<SamplerPhase>({ status: "closed" });
  const error = signal<string>("");
  const expanded = signal<ReadonlySet<string>>(new Set());
  const selected = signal<string | null>(null);
  const pending = signal<ReadonlySet<string>>(new Set());
  const visible = signal<readonly string[]>([]);
  let knownEnabled: ReadonlySet<string> = new Set();
  let generation = 0;
  let prioritySignature = "";
  let selectionRevision = 0;
  let selections: Promise<void> = Promise.resolve();
  let disposed = false;
  const job = computed((): SamplerJob | null => {
    const current = phase.value;
    return current.status === "ready" ? current.job : null;
  });
  const state = computed((): SamplerState => {
    const current = phase.value;
    return {
      samplerOpen: current.status !== "closed",
      samplerLoading: current.status === "opening",
      samplerError: current.status === "failed" ? current.error : error.value,
      samplerJob: job.value,
      samplerExpandedSections: expanded.value,
      samplerSelectedKey: selected.value,
      samplerPendingSelections: pending.value,
    };
  });

  /** Reveal newly enabled branches while preserving the user's existing expansion choices. */
  function receiveJob(next: SamplerJob | null): void {
    if (!next) throw new Error("sampler job returned no data");
    const enabled = new Set(
      next.entries.filter((entry) => entry.current_enabled || entry.selected).map((entry) => entry.key),
    );
    const sections = new Set(expanded.peek());
    const hierarchy = buildSamplerHierarchy(next.entries);
    for (const key of enabled) {
      if (knownEnabled.has(key)) continue;
      const section = hierarchy.entrySections.get(key);
      if (!section) continue;
      sections.add(section.key);
      section.ancestorKeys.forEach((ancestor) => sections.add(ancestor));
    }
    knownEnabled = enabled;
    batch((): void => {
      phase.value = { status: "ready", job: next };
      expanded.value = sections;
    });
  }

  /** Neutral sampling consumes no review edits; synchronous ownership prevents duplicate starts. */
  async function openSampler(): Promise<void> {
    if (disposed || phase.peek().status !== "closed") return;
    const imageId = catalog.field("currentId").peek();
    if (imageId === null || !catalog.confirmedImage(imageId) || !catalog.catalogField("capabilities").peek()?.sampler)
      return;
    const request = ++generation;
    prioritySignature = "";
    knownEnabled = new Set();
    batch((): void => {
      phase.value = { status: "opening" };
      error.value = "";
      expanded.value = new Set();
      selected.value = null;
      pending.value = new Set();
      visible.value = [];
    });
    try {
      const next = await reviewApi.sampler_create({ body: { image_id: imageId } });
      if (disposed || generation !== request) return;
      receiveJob(next);
      selected.value =
        next?.entries.find((entry) => entry.selected)?.key ??
        next?.entries.find((entry) => entry.current_enabled)?.key ??
        null;
    } catch (cause: unknown) {
      if (!disposed && generation === request) phase.value = { status: "failed", error: errorMessage(cause) };
    }
  }

  /** Invalidate completions before hiding; already issued mutations remain owned by the server. */
  function closeSampler(): void {
    generation += 1;
    phase.value = { status: "closed" };
  }

  const pollingId = computed((): number | null => {
    const current = job.value;
    return current && current.status !== "done" && current.status !== "failed" ? current.id : null;
  });
  /** Poll by stable identity without restarting the delay for each progress snapshot. */
  effect(() => {
    const jobId = pollingId.value;
    if (jobId === null) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    /** Retry read failures serially; obsolete reads cannot update a later dialog. */
    const poll = async (): Promise<void> => {
      const revision = selectionRevision;
      try {
        if (pending.peek().size === 0) {
          const next = await reviewApi.sampler_get({ params: { job_id: jobId }, signal: controller.signal });
          if (controller.signal.aborted) return;
          if (revision === selectionRevision && pending.peek().size === 0) {
            receiveJob(next);
            error.value = "";
          }
        }
      } catch (cause: unknown) {
        if (controller.signal.aborted || isAbortError(cause)) return;
        if (revision === selectionRevision && pending.peek().size === 0) error.value = errorMessage(cause);
      }
      if (!controller.signal.aborted) timer = setTimeout(() => void poll(), SAMPLER_POLL_MS);
    };
    timer = setTimeout(() => void poll(), SAMPLER_POLL_MS);
    return (): void => {
      clearTimeout(timer);
      controller.abort();
    };
  });

  /** Select only the comparison preview, leaving profile availability untouched. */
  function selectSamplerEntry(key: string): void {
    selected.value = key;
  }

  /** Controlled details elements report expansion through one action. */
  function toggleSamplerSection(key: string, open: boolean): void {
    if (expanded.peek().has(key) === open) return;
    const next = new Set(expanded.peek());
    if (open) next.add(key);
    else next.delete(key);
    expanded.value = next;
  }

  /** Enabling commits affected edits first; disabling needs no review-edit barrier. */
  async function updateSamplerSelection(entry: SamplerEntry, scope: DiffusionScope, enabled: boolean): Promise<void> {
    const current = job.peek();
    const key = `${entry.key}:${scope}`;
    if (!current || entry.status !== "done" || pending.peek().has(key)) return;
    const request = generation;
    selectionRevision += 1;
    batch((): void => {
      error.value = "";
      pending.value = new Set([...pending.peek(), key]);
    });
    /** Closing does not abort this server mutation or release its edit queue ownership. */
    const select = async (): Promise<SamplerJob | null> =>
      reviewApi.sampler_select({
        params: { job_id: current.id, entry_key: entry.key },
        body: { scope, enabled },
      });
    // Capture the edit cut now. Its callback waits only for older sampler work, never a future queue entry.
    const previous = selections;
    const selection = enabled
      ? session.commit(scope === "all" ? "all" : [current.image_id], async (): Promise<SamplerJob | null> => {
          await previous;
          return select();
        })
      : previous.then(select);
    selections = selection.then(
      (): void => {},
      (): void => {},
    );
    try {
      const next = await selection;
      if (disposed || generation !== request) return;
      receiveJob(next);
      if (enabled) selected.value = entry.key;
    } catch (cause: unknown) {
      if (!disposed && generation === request) error.value = errorMessage(cause);
    } finally {
      if (!disposed && generation === request) {
        const next = new Set(pending.peek());
        next.delete(key);
        pending.value = next;
      }
    }
  }

  /** Accept viewport measurements without granting the model DOM mutation ownership. */
  function setVisibleEntries(keys: readonly string[]): void {
    const next = [...keys].sort();
    if (next.length === visible.peek().length && next.every((key, index) => key === visible.peek()[index])) return;
    visible.value = next;
  }

  let previousPriority: SamplerPriorityInput | null = null;
  const priorityInput = computed((): SamplerPriorityInput | null => {
    const current = job.value;
    if (!current) {
      previousPriority = null;
      return null;
    }
    const sections = expanded.value;
    const keys = new Set<string>();
    /** Descendants are eligible only while every ancestor is expanded. */
    function visit(section: SamplerSectionData, parentsExpanded: boolean): void {
      const open = parentsExpanded && sections.has(section.key);
      if (open) section.entries.forEach((entry) => keys.add(entry.key));
      section.children.forEach((child) => visit(child, open));
    }
    buildSamplerHierarchy(current.entries).sections.forEach((section) => visit(section, true));
    const body = { visible_keys: [...visible.value], expanded_keys: [...keys].sort() };
    const signature = `${current.id}|${JSON.stringify(body)}`;
    if (previousPriority?.signature !== signature) previousPriority = { jobId: current.id, body, signature };
    return previousPriority;
  });

  /** Debounce real input changes; equal progress snapshots retain the same pending or acknowledged request. */
  effect(() => {
    const input = priorityInput.value;
    if (!input || input.signature === prioritySignature) return;
    const { signature, body, jobId } = input;
    const controller = new AbortController();
    let inFlight = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    /** Retry only this disposable priority update; job creation and profile selection are never replayed. */
    async function sendPriority(): Promise<void> {
      if (controller.signal.aborted) return;
      inFlight = true;
      prioritySignature = signature;
      try {
        await reviewApi.sampler_priority({ params: { job_id: jobId }, body, signal: controller.signal });
      } catch (cause: unknown) {
        if (controller.signal.aborted || isAbortError(cause)) return;
        if (prioritySignature === signature) prioritySignature = "";
        error.value = errorMessage(cause);
        timer = setTimeout((): void => {
          void sendPriority();
        }, SAMPLER_POLL_MS);
      } finally {
        inFlight = false;
      }
    }
    timer = setTimeout((): void => {
      void sendPriority();
    }, SAMPLER_PRIORITY_DEBOUNCE_MS);
    return (): void => {
      clearTimeout(timer);
      if (inFlight && prioritySignature === signature) prioritySignature = "";
      controller.abort();
    };
  });

  /** Keep the historical comparison fallback while a selected render is pending. */
  function samplerSelectedEntry(current: SamplerJob | null): SamplerEntry | null {
    const entries = current?.entries ?? [];
    return (
      entries.find((entry) => entry.key === selected.value && entry.status === "done") ??
      entries.find((entry) => entry.current_enabled && entry.status === "done") ??
      entries.find((entry) => entry.status === "done") ??
      null
    );
  }

  /** Invalidate mutation completions on provider disposal, not passive view cleanup. */
  effect(() => (): void => {
    disposed = true;
    generation += 1;
  });
  return {
    state,
    openSampler,
    closeSampler,
    selectSamplerEntry,
    toggleSamplerSection,
    updateSamplerSelection,
    samplerSelectedEntry,
    setVisibleEntries,
  };
});
