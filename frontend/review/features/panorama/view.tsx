/**
 * Reactive panorama views render controlled tool state; stable component identities preserve focus and open details.
 */
import { createContext } from "preact";
import { useContext } from "preact/hooks";
import type { ToolsController } from "../../tools/use-tools";
import type { ComponentChildren } from "preact";

import type { ReviewState, PanoramaMatching } from "../../core/types";
import { PANORAMA_MATCHING_MODES, PANORAMA_PROJECTIONS } from "../../core/constants";
import { capitalize, versionedUrl } from "../../core/selectors";
import { panoramaStatusText } from "./helpers";

/** Project state and typed workflow actions supplied to the panorama wizard. */
export interface PanoramaViewDependencies {
  closePanoramaWizard: ToolsController["closePanoramaWizard"];
  currentPanoramaProject: ToolsController["currentPanoramaProject"];
  generatePanoramaPreviews: ToolsController["generatePanoramaPreviews"];
  minRating: ToolsController["minRating"];
  movePanoramaSource: ToolsController["movePanoramaSource"];
  renderPanoramaFinal: ToolsController["renderPanoramaFinal"];
  updatePanorama: ToolsController["updatePanorama"];
  selectPanoramaProject: ToolsController["selectPanoramaProject"];
  state: ReviewState;
  togglePanoramaSource: ToolsController["togglePanoramaSource"];
  updateSharedUi: ToolsController["updateSharedUi"];
}

export const PanoramaViewContext = createContext<PanoramaViewDependencies | null>(null);

/** Read the current dialog dependencies from its provider instead of retaining initial factory closures. */
function usePanoramaView(): PanoramaViewDependencies {
  const value = useContext(PanoramaViewContext);
  if (!value) throw new Error("Panorama views require their tool provider");
  return value;
}

/** Render panorama overlay from current state and typed callbacks. */
export function PanoramaOverlay(): ComponentChildren {
  const {
    closePanoramaWizard,
    currentPanoramaProject,
    generatePanoramaPreviews,
    minRating,
    movePanoramaSource,
    renderPanoramaFinal,
    selectPanoramaProject,
    state,
    togglePanoramaSource,
    updateSharedUi,
    updatePanorama,
  } = usePanoramaView();
  const project = currentPanoramaProject();
  const projects = state.data?.panorama?.projects || [];
  const images = state.data?.images || [];
  const busy = Boolean(state.data?.panorama?.busy);
  const operationRunning = ["previewing", "rendering"].includes(project?.status || "");
  const anyProjectRunning = projects.some((candidate) => ["previewing", "rendering"].includes(candidate.status));
  const selected = new Map(state.panoramaImageIds.map((imageId, index) => [imageId, index]));
  const previews = new Map(
    (project?.previews || [])
      .filter((preview) => preview.matching_mode === state.panoramaMatching)
      .map((preview) => [preview.projection, preview]),
  );
  const selectedPreview = previews.get(state.panoramaProjection);
  const canPreview = state.panoramaImageIds.length >= 2 && !busy && !anyProjectRunning;
  const canRender =
    selectedPreview?.status === "done" && project?.status !== "complete" && !anyProjectRunning && !operationRunning;
  const progressTotal = Math.max(1, Number(project?.progress_total) || 1);
  const progressValue = Math.min(progressTotal, Number(project?.progress_completed) || 0);

  return (
    <section class={"panorama-card"}>
      <header class={"panorama-header"}>
        <div>
          <h2 id={"panorama-title"}>{"Panorama"}</h2>
          <p>{`${state.panoramaImageIds.length} selected | ${project ? capitalize(project.status) : "New project"}`}</p>
        </div>
        <div class={"panorama-header-actions"}>
          <select
            value={state.panoramaProjectId === null ? "new" : String(state.panoramaProjectId)}
            aria-label={"Panorama project"}
            onChange={(event) => selectPanoramaProject(event.currentTarget.value)}
          >
            <option value={"new"}>{"New panorama"}</option>
            {projects.map((candidate) => (
              <option key={candidate.id} value={String(candidate.id)}>
                {`${candidate.name} - ${candidate.status}`}
              </option>
            ))}
          </select>
          <button type={"button"} class={"panorama-close"} aria-label={"Close panorama"} onClick={closePanoramaWizard}>
            {"×"}
          </button>
        </div>
      </header>
      <div class={"panorama-layout"}>
        <section class={"panorama-sources"}>
          <h3>{"Sources"}</h3>
          <div class={"panorama-source-list"}>
            {images.map((image) => {
              const position = selected.get(image.id);
              const checked = position !== undefined;
              const thumb = image.thumbnail_url || image.preview_url;
              return (
                <label key={image.id} class={`panorama-source ${checked ? "selected" : ""}`}>
                  <input
                    type={"checkbox"}
                    checked={checked}
                    disabled={operationRunning}
                    onChange={() => togglePanoramaSource(image.id)}
                  />
                  <span class={"panorama-source-order"}>{checked ? String(position + 1) : ""}</span>
                  {thumb ? (
                    <img
                      src={versionedUrl(thumb, image.preview_updated_at || image.updated_at)}
                      alt={""}
                      loading={"lazy"}
                      decoding={"async"}
                    />
                  ) : (
                    <span class={"panorama-source-placeholder"} />
                  )}
                  <span class={"panorama-source-name"} title={image.relative_path}>
                    {image.file_name}
                  </span>
                  {checked ? (
                    <span class={"panorama-source-move"}>
                      <button
                        type={"button"}
                        title={"Move earlier"}
                        aria-label={`Move ${image.file_name} earlier`}
                        disabled={position === 0 || operationRunning}
                        onClick={(event) => {
                          event.preventDefault();
                          movePanoramaSource(image.id, -1);
                        }}
                      >
                        {"↑"}
                      </button>
                      <button
                        type={"button"}
                        title={"Move later"}
                        aria-label={`Move ${image.file_name} later`}
                        disabled={position === state.panoramaImageIds.length - 1 || operationRunning}
                        onClick={(event) => {
                          event.preventDefault();
                          movePanoramaSource(image.id, 1);
                        }}
                      >
                        {"↓"}
                      </button>
                    </span>
                  ) : null}
                </label>
              );
            })}
          </div>
        </section>
        <section class={"panorama-workflow"}>
          <div class={"panorama-settings"}>
            <label>
              <span>{"Name"}</span>
              <input
                type={"text"}
                value={state.panoramaName}
                disabled={operationRunning}
                autocomplete={"off"}
                onInput={(event) => {
                  updatePanorama({ panoramaName: event.currentTarget.value });
                }}
              />
            </label>
            <label>
              <span>{"Matching"}</span>
              <select
                value={state.panoramaMatching}
                disabled={operationRunning}
                onChange={(event) => {
                  updatePanorama({ panoramaMatching: event.currentTarget.value as PanoramaMatching });
                }}
              >
                {PANORAMA_MATCHING_MODES.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <button
              type={"button"}
              disabled={!canPreview}
              onClick={() => {
                void generatePanoramaPreviews();
              }}
            >
              {project?.previews?.length ? "Regenerate previews" : "Generate previews"}
            </button>
          </div>
          <div class={"panorama-projections"}>
            {PANORAMA_PROJECTIONS.map(([value, label]) => {
              const preview = previews.get(value);
              const active = state.panoramaProjection === value;
              return (
                <button
                  key={value}
                  type={"button"}
                  class={`panorama-projection ${active ? "active" : ""}`}
                  aria-pressed={active}
                  disabled={preview?.status !== "done"}
                  onClick={() => {
                    updatePanorama({ panoramaProjection: value });
                  }}
                >
                  <span class={"panorama-projection-media"}>
                    {preview?.url ? (
                      <img
                        src={versionedUrl(preview.url, preview.updated_at)}
                        alt={`${label} panorama preview`}
                        loading={"lazy"}
                        decoding={"async"}
                      />
                    ) : (
                      <span>{preview ? capitalize(preview.status) : "Not rendered"}</span>
                    )}
                  </span>
                  <span class={"panorama-projection-label"}>{label}</span>
                </button>
              );
            })}
          </div>
        </section>
      </div>
      <footer class={"panorama-footer"}>
        <div class={"panorama-status"} role="status" aria-live="polite">
          {operationRunning ? (
            <>
              <span>{panoramaStatusText(project)}</span>
              <progress max={progressTotal} value={progressValue} aria-label="Panorama rendering progress" />
            </>
          ) : (
            <span class={project?.error ? "error" : ""}>
              {state.panoramaMessage || project?.error || panoramaStatusText(project)}
            </span>
          )}
        </div>
        <div class={"panorama-footer-actions"}>
          {project?.result_image_id ? (
            <button
              type={"button"}
              onClick={() => {
                updateSharedUi({ current_image_id: project.result_image_id, min_rating: minRating() }).catch((error) =>
                  console.error(error),
                );
                closePanoramaWizard();
              }}
            >
              {"Open result"}
            </button>
          ) : null}
          <button
            type={"button"}
            class={"panorama-render"}
            disabled={!canRender}
            onClick={() => {
              void renderPanoramaFinal();
            }}
          >
            {"Render full TIFF"}
          </button>
        </div>
      </footer>
    </section>
  );
}
