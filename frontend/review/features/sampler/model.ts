/** The sampler hook owns catalog requests, polling, and viewport priorities so closing a dialog cancels its work. */
import { useCallback, useEffect, useRef } from "preact/hooks";
import type { RefObject } from "preact";
import { useReviewContext } from "../../core/context";
import { reviewApi } from "../../core/api";
import type { DiffusionScope, SamplerEntry, SamplerJob } from "../../core/types";
import { SAMPLER_POLL_MS, SAMPLER_PRIORITY_DEBOUNCE_MS } from "../../core/constants";
import { buildSamplerHierarchy, type SamplerSectionData } from "./helpers";
import { errorMessage, isAbortError } from "../../tools/common";

/** Catalog actions and a scoped viewport ref consumed by the sampler overlay. */
export interface SamplerActions {
  samplerRootRef: RefObject<HTMLDivElement>;
  openSampler(this: void): Promise<void>;
  closeSampler(this: void): void;
  selectSamplerEntry(this: void, key: string): void;
  toggleSamplerSection(this: void, key: string, expanded: boolean): void;
  updateSamplerSelection(this: void, entry: SamplerEntry, scope: DiffusionScope, enabled: boolean): Promise<void>;
  samplerSelectedEntry(this: void, job: SamplerJob | null): SamplerEntry | null;
}

/** Coordinate sampler activity with the current reactive snapshot and cancel stale asynchronous completions. */
export function useSampler(): SamplerActions {
  const { state, update } = useReviewContext();
  const latest = useRef(state);
  latest.current = state;
  const generation = useRef(0);
  const prioritySignature = useRef("");
  const samplerRootRef = useRef<HTMLDivElement>(null);

  /** Merge a fresh catalog while automatically opening newly enabled profile branches. */
  const receiveJob = useCallback(
    (job: SamplerJob | null): void => {
      update((current) => {
        const enabled = new Set(
          (job?.entries || []).filter((entry) => entry.current_enabled || entry.selected).map((entry) => entry.key),
        );
        const expanded = new Set(current.samplerExpandedSections);
        const hierarchy = buildSamplerHierarchy(job?.entries || []);
        for (const key of enabled) {
          if (current.samplerKnownEnabledKeys.has(key)) continue;
          const section = hierarchy.entrySections.get(key);
          if (!section) continue;
          expanded.add(section.key);
          section.ancestorKeys.forEach((ancestor) => expanded.add(ancestor));
        }
        return { samplerJob: job, samplerKnownEnabledKeys: enabled, samplerExpandedSections: expanded };
      });
    },
    [update],
  );

  /** Start a catalog for the selected picture; ignore responses from an earlier dialog session. */
  const openSampler = useCallback(async (): Promise<void> => {
    const current = latest.current;
    const image = current.data?.images.find((candidate) => candidate.id === current.currentId);
    if (!image || !current.data?.capabilities.sampler) return;
    const request = ++generation.current;
    prioritySignature.current = "";
    update({
      samplerOpen: true,
      samplerLoading: true,
      samplerError: "",
      samplerJob: null,
      samplerExpandedSections: new Set(),
      samplerKnownEnabledKeys: new Set(),
      samplerSelectedKey: null,
      samplerPendingSelections: new Set(),
    });
    try {
      const job = await reviewApi.sampler_create({ body: { image_id: image.id } });
      if (generation.current !== request) return;
      if (!job) throw new Error("sampler job returned no data");
      receiveJob(job);
      update({
        samplerSelectedKey:
          job.entries.find((entry) => entry.selected)?.key ||
          job.entries.find((entry) => entry.current_enabled)?.key ||
          null,
        samplerLoading: false,
      });
    } catch (error) {
      if (generation.current === request) update({ samplerLoading: false, samplerError: errorMessage(error) });
    }
  }, [update, receiveJob]);

  /** Closing invalidates outstanding catalog and selection responses before the overlay unmounts. */
  const closeSampler = useCallback((): void => {
    generation.current += 1;
    update({ samplerOpen: false, samplerLoading: false });
  }, [update]);

  useEffect(() => {
    if (!state.samplerOpen || !state.samplerJob || ["done", "failed"].includes(state.samplerJob.status)) return;
    const jobId = state.samplerJob.id;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    /** Retry transport failures without overlapping requests or depending on error text changing. */
    const poll = async (): Promise<void> => {
      try {
        const job = await reviewApi.sampler_get({ params: { job_id: jobId }, signal: controller.signal });
        if (!controller.signal.aborted) {
          receiveJob(job);
          update({ samplerError: "" });
          if (job && !["done", "failed"].includes(job.status))
            timer = setTimeout(() => {
              void poll();
            }, SAMPLER_POLL_MS);
        }
      } catch (error) {
        if (!isAbortError(error) && !controller.signal.aborted) {
          update({ samplerError: errorMessage(error) });
          timer = setTimeout(() => {
            void poll();
          }, SAMPLER_POLL_MS);
        }
      }
    };
    timer = setTimeout(() => {
      void poll();
    }, SAMPLER_POLL_MS);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [state.samplerOpen, state.samplerJob, update, receiveJob]);

  /** Change the preview selection without changing the enabled profile set. */
  const selectSamplerEntry = useCallback((key: string): void => update({ samplerSelectedKey: key }), [update]);

  /** Store expansion through the reducer so details elements stay controlled after updates. */
  const toggleSamplerSection = useCallback(
    (key: string, expanded: boolean): void => {
      if (latest.current.samplerExpandedSections.has(key) === expanded) return;
      update((current) => {
        const sections = new Set(current.samplerExpandedSections);
        if (expanded) sections.add(key);
        else sections.delete(key);
        return { samplerExpandedSections: sections };
      });
    },
    [update],
  );

  /** Apply availability to this picture or all pictures and merge the returned catalog. */
  const updateSamplerSelection = useCallback(
    async (entry: SamplerEntry, scope: DiffusionScope, enabled: boolean): Promise<void> => {
      const jobId = latest.current.samplerJob?.id;
      if (!jobId || entry.status !== "done") return;
      const request = generation.current;
      const key = `${entry.key}:${scope}`;
      update((current) => ({
        samplerError: "",
        samplerPendingSelections: new Set([...current.samplerPendingSelections, key]),
      }));
      try {
        const job = await reviewApi.sampler_select({
          params: { job_id: jobId, entry_key: entry.key },
          body: { scope, enabled },
        });
        if (generation.current !== request) return;
        receiveJob(job);
        if (enabled) update({ samplerSelectedKey: entry.key });
      } catch (error) {
        if (generation.current === request) update({ samplerError: errorMessage(error) });
      } finally {
        if (generation.current === request)
          update((current) => {
            const pending = new Set(current.samplerPendingSelections);
            pending.delete(key);
            return { samplerPendingSelections: pending };
          });
      }
    },
    [update, receiveJob],
  );

  useEffect(() => {
    const root = samplerRootRef.current;
    const job = state.samplerJob;
    if (!root || !state.samplerOpen || !job) return;
    const visible = new Set<string>();
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    /** Debounce viewport changes and avoid repeating the same visible/expanded priority request. */
    const sendPriority = (): void => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const keys = new Set<string>();
        /** Include expanded ancestors before making descendants render priorities. */
        const visit = (section: SamplerSectionData, parentsExpanded: boolean): void => {
          const expanded = parentsExpanded && latest.current.samplerExpandedSections.has(section.key);
          if (expanded) section.entries.forEach((entry) => keys.add(entry.key));
          section.children.forEach((child) => visit(child, expanded));
        };
        buildSamplerHierarchy(latest.current.samplerJob?.entries || []).sections.forEach((section) =>
          visit(section, true),
        );
        const body = { visible_keys: [...visible].sort(), expanded_keys: [...keys].sort() };
        const signature = `${job.id}|${JSON.stringify(body)}`;
        if (signature === prioritySignature.current) return;
        prioritySignature.current = signature;
        void reviewApi
          .sampler_priority({
            params: { job_id: job.id },
            body,
            signal: controller.signal,
          })
          .catch((error: unknown) => {
            if (prioritySignature.current === signature) prioritySignature.current = "";
            if (!isAbortError(error)) console.error(error);
          });
      }, SAMPLER_PRIORITY_DEBOUNCE_MS);
    };
    const observer =
      typeof IntersectionObserver === "undefined"
        ? null
        : new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                const key = (entry.target as HTMLElement).dataset["samplerKey"];
                if (!key) continue;
                if (entry.isIntersecting) visible.add(key);
                else visible.delete(key);
              }
              sendPriority();
            },
            { root, rootMargin: "80px 0px", threshold: 0.01 },
          );
    root.querySelectorAll<HTMLElement>("[data-sampler-key]").forEach((tile) => observer?.observe(tile));
    sendPriority();
    return () => {
      clearTimeout(timer);
      observer?.disconnect();
      controller.abort();
    };
  }, [state.samplerOpen, state.samplerJob, state.samplerExpandedSections]);

  /** Prefer the explicitly selected completed render, then an enabled render, then the first result. */
  const samplerSelectedEntry = (job: SamplerJob | null): SamplerEntry | null => {
    const entries = job?.entries || [];
    return (
      entries.find((entry) => entry.key === state.samplerSelectedKey && entry.status === "done") ||
      entries.find((entry) => entry.current_enabled && entry.status === "done") ||
      entries.find((entry) => entry.status === "done") ||
      null
    );
  };
  return {
    samplerRootRef,
    openSampler,
    closeSampler,
    selectSamplerEntry,
    toggleSamplerSection,
    updateSamplerSelection,
    samplerSelectedEntry,
  };
}
