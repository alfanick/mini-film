/**
 * Reactive diffusion views render controlled tool state; stable component identities preserve focus and open details.
 */
import { createContext } from "preact";
import { useContext } from "preact/hooks";
import type { ToolsController } from "../../tools/use-tools";
import {
  formatPercent,
  diffusionAfterSource,
  diffusionDetailFrameStyle,
  diffusionDetailMediaStyle,
  diffusionPresetIsActive,
  diffusionPresetSettings,
  diffusionProfile,
  diffusionSourceLabel,
  normalizeDiffusionSettings,
} from "./helpers";
import { DIFFUSION_DETAIL_AREAS, DIFFUSION_METHODS, DIFFUSION_PRESETS } from "../../core/constants";
import { profileDisplayName, versionedUrl } from "../../core/selectors";
import { reviewUrl } from "../../core/api";
import type { ComponentChildren } from "preact";

import type {
  ReviewState,
  DiffusionJob,
  DiffusionSettings,
  DiffusionPreviewContext,
  DiffusionDetailArea,
} from "../../core/types";

/** Current diffusion state and lifecycle actions for the comparison components. */
export interface DiffusionViewDependencies {
  applyDiffusion: ToolsController["applyDiffusion"];
  closeDiffusion: ToolsController["closeDiffusion"];
  diffusionBeforeSource: ToolsController["diffusionBeforeSource"];
  diffusionMediaStyle: ToolsController["diffusionMediaStyle"];
  diffusionPreviewContext: ToolsController["diffusionPreviewContext"];
  diffusionStatusText: ToolsController["diffusionStatusText"];
  findImage: ToolsController["findImage"];
  requestDiffusionPreview: ToolsController["requestDiffusionPreview"];
  resetDiffusion: ToolsController["resetDiffusion"];
  setDiffusionSettings: ToolsController["setDiffusionSettings"];
  state: ReviewState;
}

interface DiffusionComparisonProps {
  before: ReturnType<DiffusionViewDependencies["diffusionBeforeSource"]>;
  after: ReturnType<typeof diffusionAfterSource>;
  mediaStyle: ReturnType<DiffusionViewDependencies["diffusionMediaStyle"]>;
  job: DiffusionJob | null;
}

interface DiffusionDetailComparisonsProps {
  before: DiffusionComparisonProps["before"];
  after: DiffusionComparisonProps["after"];
  previewContext: DiffusionPreviewContext | null;
  job: DiffusionJob | null;
}

interface DiffusionDetailFigureProps {
  label: string;
  source: DiffusionComparisonProps["before"];
  area: DiffusionDetailArea | null;
  previewContext: DiffusionPreviewContext | null;
  placeholder: string;
  alt: string;
}

interface DiffusionSliderProps {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  disabled: boolean;
  onInput: (value: number) => void;
  formatValue?: (value: number) => string;
}

export const DiffusionViewContext = createContext<DiffusionViewDependencies | null>(null);

/** Read the current dialog dependencies from its provider instead of retaining initial factory closures. */
function useDiffusionView(): DiffusionViewDependencies {
  const value = useContext(DiffusionViewContext);
  if (!value) throw new Error("Diffusion views require their tool provider");
  return value;
}

/** Render diffusion overlay from current state and typed callbacks. */
export function DiffusionOverlay(): ComponentChildren {
  const {
    applyDiffusion,
    closeDiffusion,
    diffusionBeforeSource,
    diffusionMediaStyle,
    diffusionPreviewContext,
    diffusionStatusText,
    findImage,
    requestDiffusionPreview,
    resetDiffusion,
    state,
  } = useDiffusionView();
  const image = findImage(state.diffusionImageId);
  const profile = diffusionProfile(image, state.diffusionProfileIndex);
  const settings = normalizeDiffusionSettings(state.diffusionSettings);
  const job = state.diffusionJob;
  const before = diffusionBeforeSource(job);
  const after = diffusionAfterSource(job);
  const mediaStyle = diffusionMediaStyle(job, image, profile);
  const previewContext = diffusionPreviewContext(job);
  const controlsDisabled = state.diffusionSaving;
  const sourceLabel = diffusionSourceLabel(state.diffusionSource);
  return (
    <section class={"diffusion-card"}>
      <header class={"diffusion-header"}>
        <div>
          <h2 id={"diffusion-title"}>{"Diffusion"}</h2>
          <p>
            {image && profile
              ? `${image.file_name} | ${profileDisplayName(profile)}${sourceLabel ? ` | ${sourceLabel}` : ""}`
              : "Film-like softness and highlight glow"}
          </p>
        </div>
        <button
          type={"button"}
          class={"diffusion-close"}
          aria-label={"Cancel diffusion changes"}
          disabled={controlsDisabled}
          onClick={closeDiffusion}
        >
          {"×"}
        </button>
      </header>
      <main class={"diffusion-body"}>
        <div class={"diffusion-workspace"}>
          <DiffusionFullComparison before={before} after={after} mediaStyle={mediaStyle} job={job} />
          <DiffusionDetailComparisons before={before} after={after} previewContext={previewContext} job={job} />
        </div>
        <aside class={"diffusion-control-rail"} aria-label={"Diffusion controls"}>
          <DiffusionControls settings={settings} controlsDisabled={controlsDisabled} />
          <div class={`diffusion-status ${state.diffusionError ? "error" : ""}`} role={"status"} aria-live={"polite"}>
            <span>{state.diffusionError || state.diffusionMessage || diffusionStatusText(job)}</span>
            {state.diffusionError && state.diffusionErrorKind === "preview" && !state.diffusionSaving ? (
              <button type={"button"} onClick={requestDiffusionPreview}>
                {"Retry preview"}
              </button>
            ) : state.diffusionLoading ? (
              <progress max={1} aria-label="Preparing diffusion preview" />
            ) : null}
          </div>
          <p class={"diffusion-scope-note"}>
            {"All applies this diffusion setting to the current profile for every existing and future picture."}
          </p>
          <footer class={"diffusion-footer"}>
            <div class={"diffusion-reset-actions"}>
              <button
                type={"button"}
                disabled={controlsDisabled}
                title={"Remove the current picture override and inherit this profile's all-picture setting"}
                onClick={() => {
                  void resetDiffusion("current");
                }}
              >
                {"Reset current"}
              </button>
              <button
                type={"button"}
                disabled={controlsDisabled}
                title={"Remove this profile's setting for all existing and future pictures"}
                onClick={() => {
                  void resetDiffusion("all");
                }}
              >
                {"Reset all"}
              </button>
            </div>
            <div class={"diffusion-apply-actions"}>
              <button type={"button"} disabled={controlsDisabled} onClick={closeDiffusion}>
                {"Cancel"}
              </button>
              <button
                type={"button"}
                class={"diffusion-apply"}
                disabled={controlsDisabled || state.diffusionLoading}
                title={"Apply these settings only to the current picture and profile"}
                onClick={() => {
                  void applyDiffusion("current");
                }}
              >
                {"Apply to current"}
              </button>
              <button
                type={"button"}
                class={"diffusion-apply"}
                disabled={controlsDisabled || state.diffusionLoading}
                title={"Apply to this profile across all existing and future pictures"}
                onClick={() => {
                  void applyDiffusion("all");
                }}
              >
                {"Apply to all"}
              </button>
            </div>
          </footer>
        </aside>
      </main>
    </section>
  );
}

/** Render diffusion full comparison from current state and typed callbacks. */
export function DiffusionFullComparison({
  before,
  after,
  mediaStyle,
  job,
}: DiffusionComparisonProps): ComponentChildren {
  const { diffusionStatusText, state } = useDiffusionView();
  return (
    <section class={"diffusion-comparison"} aria-label={"Diffusion before and after preview"}>
      <figure>
        {before.url ? (
          <img
            src={versionedUrl(reviewUrl(before.url), before.updatedAt)}
            alt={"Before diffusion"}
            style={mediaStyle}
            decoding={"async"}
          />
        ) : (
          <div class={"diffusion-preview-placeholder"} style={mediaStyle}>
            {"Preparing source"}
          </div>
        )}
        <figcaption>{"Before"}</figcaption>
      </figure>
      <figure>
        {after.url ? (
          <img
            src={versionedUrl(reviewUrl(after.url), after.updatedAt)}
            alt={"After diffusion"}
            style={mediaStyle}
            decoding={"async"}
          />
        ) : (
          <div class={"diffusion-preview-placeholder"} style={mediaStyle}>
            {state.diffusionError || diffusionStatusText(job)}
          </div>
        )}
        <figcaption>{"After"}</figcaption>
      </figure>
    </section>
  );
}

/** Render diffusion detail comparisons from current state and typed callbacks. */
export function DiffusionDetailComparisons({
  before,
  after,
  previewContext,
  job,
}: DiffusionDetailComparisonsProps): ComponentChildren {
  const { diffusionStatusText, state } = useDiffusionView();
  return (
    <section class={"diffusion-details"} aria-labelledby={"diffusion-details-title"}>
      <header class={"diffusion-details-header"}>
        <h3 id={"diffusion-details-title"}>{"Detail comparisons"}</h3>
        <p>{"Automatically selected from the source preview"}</p>
      </header>
      <div
        class={"diffusion-detail-strip"}
        tabIndex={0}
        aria-label={"Automatically selected diffusion detail comparisons"}
      >
        {DIFFUSION_DETAIL_AREAS.map((definition) => {
          const area = previewContext?.areas.find((candidate) => candidate.kind === definition.kind) || null;
          const note =
            definition.kind === "focus" && area
              ? previewContext?.focusSource === "center-fallback"
                ? "Center fallback"
                : previewContext?.focusSource === "camera-focus"
                  ? "Camera focus"
                  : ""
              : "";
          return (
            <article key={definition.kind} class={"diffusion-detail-card"}>
              <header>
                <h4>{definition.label}</h4>
                {note ? <span>{note}</span> : null}
              </header>
              <div class={"diffusion-detail-pair"}>
                <DiffusionDetailFigure
                  label={"Before"}
                  source={before}
                  area={area}
                  previewContext={previewContext}
                  placeholder={area ? "Preparing source" : "Detecting area"}
                  alt={`${definition.label} before diffusion`}
                />
                <DiffusionDetailFigure
                  label={"After"}
                  source={after}
                  area={area}
                  previewContext={previewContext}
                  placeholder={area ? state.diffusionError || diffusionStatusText(job) : "Detecting area"}
                  alt={`${definition.label} after diffusion`}
                />
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

/** Render diffusion detail figure from current state and typed callbacks. */
export function DiffusionDetailFigure({
  label,
  source,
  area,
  previewContext,
  placeholder,
  alt,
}: DiffusionDetailFigureProps): ComponentChildren {
  return (
    <figure>
      <div class={"diffusion-detail-media"} style={diffusionDetailFrameStyle(area)}>
        {source.url && area && previewContext ? (
          <img
            src={versionedUrl(reviewUrl(source.url), source.updatedAt)}
            alt={alt}
            style={diffusionDetailMediaStyle(area, previewContext)}
            decoding={"async"}
          />
        ) : (
          <span class={"diffusion-detail-placeholder"}>{placeholder}</span>
        )}
      </div>
      <figcaption>{label}</figcaption>
    </figure>
  );
}

/** Render diffusion controls from current state and typed callbacks. */
export function DiffusionControls({
  settings,
  controlsDisabled,
}: {
  settings: DiffusionSettings;
  controlsDisabled: boolean;
}): ComponentChildren {
  const { setDiffusionSettings } = useDiffusionView();
  return (
    <div class={"diffusion-controls"}>
      <section class={"diffusion-control-section"} aria-labelledby={"diffusion-method-title"}>
        <h3 id={"diffusion-method-title"}>{"Method"}</h3>
        <div class={"diffusion-method-grid"} role={"group"} aria-label={"Diffusion method"}>
          {DIFFUSION_METHODS.map((method) => (
            <button
              key={method.id}
              type={"button"}
              class={`diffusion-method-tile ${settings.method === method.id ? "active" : ""}`}
              aria-pressed={settings.method === method.id ? "true" : "false"}
              disabled={controlsDisabled}
              onClick={() => setDiffusionSettings({ method: method.id })}
            >
              <span class={"diffusion-tile-title"}>{method.label}</span>
              <span class={"diffusion-tile-description"}>{method.description}</span>
            </button>
          ))}
        </div>
      </section>
      <section class={"diffusion-control-section"} aria-labelledby={"diffusion-preset-title"}>
        <h3 id={"diffusion-preset-title"}>{"Strength"}</h3>
        <div class={"diffusion-preset-grid"} role={"group"} aria-label={"Diffusion strength preset"}>
          {DIFFUSION_PRESETS.map((preset) => {
            const active = diffusionPresetIsActive(preset, settings);
            const presetSettings = diffusionPresetSettings(preset, settings.method);
            return (
              <button
                key={preset.id}
                type={"button"}
                class={`diffusion-preset-tile ${active ? "active" : ""}`}
                aria-pressed={active ? "true" : "false"}
                disabled={controlsDisabled}
                onClick={() => setDiffusionSettings(presetSettings)}
              >
                <span class={"diffusion-tile-title"}>{preset.label}</span>
                <span class={"diffusion-tile-description"}>{preset.description}</span>
              </button>
            );
          })}
        </div>
      </section>
      <section
        class={"diffusion-control-section diffusion-parameter-group"}
        aria-labelledby={"diffusion-softening-title"}
      >
        <h3 id={"diffusion-softening-title"}>{"Softening"}</h3>
        <div class={"diffusion-sliders"}>
          <DiffusionSlider
            id={"diffusion-softness"}
            label={"Amount"}
            value={settings.softness}
            min={0}
            max={100}
            step={1}
            disabled={controlsDisabled}
            onInput={(value) => setDiffusionSettings({ softness: value })}
          />
          <DiffusionSlider
            id={"diffusion-softness-radius"}
            label={"Radius"}
            value={settings.softness_radius_percent}
            min={50}
            max={400}
            step={5}
            disabled={controlsDisabled}
            onInput={(value) => setDiffusionSettings({ softness_radius_percent: value })}
          />
        </div>
      </section>
      <section
        class={"diffusion-control-section diffusion-parameter-group"}
        aria-labelledby={"diffusion-highlights-title"}
      >
        <h3 id={"diffusion-highlights-title"}>{"Highlights"}</h3>
        <div class={"diffusion-sliders"}>
          <DiffusionSlider
            id={"diffusion-highlight-glow"}
            label={"Glow"}
            value={settings.highlight_glow}
            min={0}
            max={100}
            step={1}
            disabled={controlsDisabled}
            onInput={(value) => setDiffusionSettings({ highlight_glow: value })}
          />
          <DiffusionSlider
            id={"diffusion-glow-radius"}
            label={"Radius"}
            value={settings.glow_radius_percent}
            min={50}
            max={400}
            step={5}
            disabled={controlsDisabled}
            onInput={(value) => setDiffusionSettings({ glow_radius_percent: value })}
          />
          {settings.method === "edge-aware-glow" ? (
            <DiffusionSlider
              id={"diffusion-highlight-reach"}
              label={"Reach"}
              value={settings.highlight_reach}
              min={0}
              max={100}
              step={1}
              disabled={controlsDisabled}
              onInput={(value) => setDiffusionSettings({ highlight_reach: value })}
            />
          ) : null}
        </div>
      </section>
      <section
        class={"diffusion-control-section diffusion-parameter-group diffusion-overall-controls"}
        aria-labelledby={"diffusion-overall-title"}
      >
        <h3 id={"diffusion-overall-title"}>{"Overall"}</h3>
        <div class={"diffusion-sliders"}>
          <DiffusionSlider
            id={"diffusion-intensity"}
            label={"Intensity"}
            value={settings.intensity_percent}
            min={25}
            max={300}
            step={5}
            disabled={controlsDisabled}
            onInput={(value) => setDiffusionSettings({ intensity_percent: value })}
          />
        </div>
      </section>
    </div>
  );
}

/** Render diffusion slider from current state and typed callbacks. */
export function DiffusionSlider({
  id,
  label,
  value,
  min,
  max,
  step,
  disabled,
  onInput,
  formatValue = formatPercent,
}: DiffusionSliderProps): ComponentChildren {
  const formattedValue = formatValue(value);
  return (
    <label class={"diffusion-slider"} for={id}>
      <span>{label}</span>
      <input
        id={id}
        type={"range"}
        min={String(min)}
        max={String(max)}
        step={String(step)}
        value={String(value)}
        aria-valuetext={formattedValue}
        disabled={disabled}
        onInput={(event) => onInput(Number(event.currentTarget.value))}
      />
      <output for={id}>{formattedValue}</output>
    </label>
  );
}
