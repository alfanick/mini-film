/** Reactive sampler views render controlled tool state; stable component identities preserve focus and open details. */
import { createContext } from "preact";
import { useContext } from "preact/hooks";
import type { ToolsController } from "../tools/use-tools";
import type { ComponentChildren } from "preact";

import type { ReviewState, SamplerJob, SamplerEntry } from "../core/types";

/** Live catalog state and selection actions shared by the sampler tree and comparison components. */
export interface SamplerViewDependencies {
  samplerRootRef: ToolsController["samplerRootRef"];
  buildSamplerHierarchy: typeof import("../tools/sampler-helpers").buildSamplerHierarchy;
  capitalize: typeof import("../core/selectors").capitalize;
  closeSampler: ToolsController["closeSampler"];
  reviewUrl: typeof import("../core/api").reviewUrl;
  samplerMediaStyle: typeof import("../tools/sampler-helpers").samplerMediaStyle;
  samplerSelectedEntry: ToolsController["samplerSelectedEntry"];
  samplerStatusText: typeof import("../tools/sampler-helpers").samplerStatusText;
  selectSamplerEntry: ToolsController["selectSamplerEntry"];
  state: ReviewState;
  toggleSamplerSection: ToolsController["toggleSamplerSection"];
  updateSamplerSelection: ToolsController["updateSamplerSelection"];
}

export interface SamplerSectionData {
  key: string;
  label: string;
  depth: number;
  ancestorKeys: string[];
  entries: SamplerEntry[];
  allEntries: SamplerEntry[];
  children: SamplerSectionData[];
}

export const SamplerViewContext = createContext<SamplerViewDependencies | null>(null);

/** Read the current dialog dependencies from its provider instead of retaining initial factory closures. */
function useSamplerView(): SamplerViewDependencies {
  const value = useContext(SamplerViewContext);
  if (!value) throw new Error("Sampler views require their tool provider");
  return value;
}

/** Render sampler overlay from current state and typed callbacks. */
export function SamplerOverlay(): ComponentChildren {
  const {
    buildSamplerHierarchy,
    closeSampler,
    reviewUrl,
    samplerMediaStyle,
    samplerSelectedEntry,
    samplerStatusText,
    state,
    samplerRootRef,
  } = useSamplerView();
  const job = state.samplerJob;
  const hierarchy = buildSamplerHierarchy(job?.entries || []);
  const selectedEntry = samplerSelectedEntry(job);
  const completed = Number(job?.completed || 0);
  const total = Number(job?.total || 0);
  const progressMax = Math.max(1, total);
  const progressValue = Math.min(progressMax, completed);
  const sourceStyle = samplerMediaStyle(job);
  return (
    <section class={"sampler-card"} role={"dialog"} aria-modal={"true"} aria-labelledby={"sampler-title"}>
      <header class={"sampler-header"}>
        <div>
          <h2 id={"sampler-title"}>{"Sampler"}</h2>
          <p>
            {job
              ? `${job.file_name} | ${completed}/${total} | ${job.workers} workers`
              : state.samplerLoading
                ? "Preparing profile catalog"
                : "Profile sampler"}
          </p>
        </div>
        <button type={"button"} class={"sampler-close"} aria-label={"Close sampler"} onClick={closeSampler}>
          {"×"}
        </button>
      </header>
      {job ? (
        <div class={"sampler-progress"}>
          <progress max={progressMax} value={progressValue} />
          <span class={job.error ? "error" : ""}>{job.error || samplerStatusText(job)}</span>
        </div>
      ) : null}
      {state.samplerError ? <div class={"sampler-error"}>{state.samplerError}</div> : null}
      {job?.source_url ? (
        <section class={"sampler-comparison"} aria-label={"Sampler comparison"}>
          <figure>
            <img src={reviewUrl(job.source_url)} alt={"Neutral source"} style={sourceStyle} decoding={"async"} />
            <figcaption>{"Neutral"}</figcaption>
          </figure>
          <figure>
            <img
              src={reviewUrl(selectedEntry?.thumbnail_url || job.source_url)}
              alt={selectedEntry?.name || "Neutral source"}
              style={sourceStyle}
              decoding={"async"}
            />
            <figcaption>{selectedEntry?.name || "Select a rendered profile"}</figcaption>
          </figure>
        </section>
      ) : null}
      <div class={"sampler-sections"} ref={samplerRootRef}>
        {hierarchy.sections.map((section) => (
          <SamplerSection key={section.key} section={section} job={job} />
        ))}
      </div>
    </section>
  );
}

/** Render sampler section from current state and typed callbacks. */
export function SamplerSection({
  section,
  job,
}: {
  section: SamplerSectionData;
  job: SamplerJob | null;
}): ComponentChildren {
  const { state, toggleSamplerSection } = useSamplerView();
  const expanded = state.samplerExpandedSections.has(section.key);
  const done = section.allEntries.filter((entry) => entry.status === "done").length;
  return (
    <details
      class={`sampler-section sampler-section-depth-${Math.min(section.depth, 3)}`}
      open={expanded}
      data-sampler-section-key={section.key}
      onToggle={(event) => toggleSamplerSection(section.key, event.currentTarget.open)}
    >
      <summary>
        <span>{section.label}</span>
        <span class={"sampler-section-count"}>{`${done}/${section.allEntries.length}`}</span>
      </summary>
      {section.entries.length > 0 ? (
        <div class={"sampler-grid"}>
          {section.entries.map((entry) => (
            <SamplerTile key={entry.key} entry={entry} job={job} />
          ))}
        </div>
      ) : null}
      {section.children.length > 0 ? (
        <div class={"sampler-section-children"}>
          {section.children.map((child) => (
            <SamplerSection key={child.key} section={child} job={job} />
          ))}
        </div>
      ) : null}
    </details>
  );
}

/** Render sampler tile from current state and typed callbacks. */
export function SamplerTile({ entry, job }: { entry: SamplerEntry; job: SamplerJob | null }): ComponentChildren {
  const { capitalize, reviewUrl, samplerMediaStyle, selectSamplerEntry, state, updateSamplerSelection } =
    useSamplerView();
  const selected = state.samplerSelectedKey === entry.key;
  const ready = entry.status === "done" && Boolean(entry.thumbnail_url);
  const currentPending = state.samplerPendingSelections.has(`${entry.key}:current`);
  const allPending = state.samplerPendingSelections.has(`${entry.key}:all`);
  const sourceStyle = samplerMediaStyle(job);
  return (
    <article class={`sampler-tile ${selected ? "selected" : ""} sampler-${entry.status}`} data-sampler-key={entry.key}>
      <button
        type={"button"}
        class={"sampler-thumbnail"}
        disabled={!ready}
        title={entry.filename}
        style={sourceStyle}
        onClick={() => selectSamplerEntry(entry.key)}
      >
        {ready && entry.thumbnail_url ? (
          <img
            src={reviewUrl(entry.thumbnail_url)}
            alt={entry.name}
            style={sourceStyle}
            loading={"lazy"}
            decoding={"async"}
          />
        ) : (
          <span class={"sampler-thumbnail-placeholder"}>{capitalize(entry.status)}</span>
        )}
      </button>
      <div class={"sampler-tile-name"} title={entry.filename}>
        {entry.name}
      </div>
      {entry.error ? (
        <div class={"sampler-tile-error"} title={entry.error}>
          {"Failed"}
        </div>
      ) : null}
      <div class={"sampler-scope"} aria-label={`${entry.name} availability`}>
        <label title={"Available for the current picture"}>
          <input
            type={"checkbox"}
            checked={Boolean(entry.current_enabled)}
            disabled={!ready || currentPending}
            onChange={(event) => {
              void updateSamplerSelection(entry, "current", event.currentTarget.checked);
            }}
          />
          <span>{"Current"}</span>
        </label>
        <label
          title={
            entry.configured_from_cli
              ? "Command-line profiles remain available to all pictures"
              : "Available for all current and future pictures"
          }
        >
          <input
            type={"checkbox"}
            checked={Boolean(entry.all_enabled)}
            disabled={!ready || allPending || entry.configured_from_cli}
            onChange={(event) => {
              void updateSamplerSelection(entry, "all", event.currentTarget.checked);
            }}
          />
          <span>{"All"}</span>
        </label>
      </div>
    </article>
  );
}
