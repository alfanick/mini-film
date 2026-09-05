/**
 * Panorama editing uses immutable drafts and server snapshots so project controls remain reactive during asynchronous
 * rendering.
 */
import { useCallback } from "preact/hooks";
import { useReviewContext } from "../../core/context";
import { reviewApi, errorMessage } from "../../core/api";
import type {
  ReviewPanoramaProject,
  ReviewState,
  ReviewStateMessage,
  PanoramaMatching,
  PanoramaProjection,
} from "../../core/types";
import type { ToolSessionActions } from "../../tools/types";

/** User-editable project choices that can change independently of server progress. */
export interface PanoramaDraft {
  panoramaName?: string;
  panoramaMatching?: PanoramaMatching;
  panoramaProjection?: PanoramaProjection;
}

/** Reactive panorama selection, ordering, preview, and render commands for its wizard. */
export interface PanoramaActions {
  openPanoramaWizard(this: void): void;
  closePanoramaWizard(this: void): void;
  currentPanoramaProject(this: void): ReviewPanoramaProject | null;
  selectPanoramaProject(this: void, value: string): void;
  togglePanoramaSource(this: void, imageId: number): void;
  movePanoramaSource(this: void, imageId: number, direction: number): void;
  updatePanorama(this: void, patch: PanoramaDraft): void;
  generatePanoramaPreviews(this: void): Promise<void>;
  renderPanoramaFinal(this: void): Promise<void>;
}

/** Choose adjacent source pictures and the legacy initial name for a new project. */
function newDraft(state: ReviewState): Partial<ReviewState> {
  const images = state.data?.images || [];
  const index = Math.max(
    0,
    images.findIndex((image) => image.id === state.currentId),
  );
  let ids = images.slice(index, index + 3).map((image) => image.id);
  if (ids.length < 2) ids = images.slice(Math.max(0, images.length - 3)).map((image) => image.id);
  const stem = images[index]?.file_name.replace(/\.[^.]+$/, "") || "Panorama";
  return {
    panoramaProjectId: null,
    panoramaImageIds: ids,
    panoramaName: `${stem} panorama`,
    panoramaMatching: "automatic",
    panoramaProjection: "cylindrical",
    panoramaMessage: "",
  };
}

/** Expose panorama commands and update the shared catalog after each successful server operation. */
export function usePanorama(session: ToolSessionActions): PanoramaActions {
  const { state, update, getState } = useReviewContext();

  /** Open the existing project draft or initialize sources for the first project. */
  const openPanoramaWizard = useCallback((): void => {
    const current = getState();
    if (!current.data?.capabilities.panorama.available) return;
    update({
      ...(current.panoramaProjectId === null ? newDraft(current) : {}),
      panoramaOpen: true,
      panoramaMessage: "",
    });
  }, [getState, update]);

  /** Hide the wizard while keeping its project draft for reopening. */
  const closePanoramaWizard = useCallback((): void => update({ panoramaOpen: false }), [update]);

  /** Select a saved project or reset the wizard to its new-project defaults. */
  const selectPanoramaProject = useCallback(
    (value: string): void => {
      const current = getState();
      if (value === "new") {
        update(newDraft(current));
        return;
      }
      const project = current.data?.panorama.projects.find((candidate) => candidate.id === Number(value));
      if (!project) return;
      update({
        panoramaProjectId: project.id,
        panoramaImageIds: [...project.image_ids],
        panoramaName: project.name || "Panorama",
        panoramaMatching: project.matching_mode || "automatic",
        panoramaProjection: project.selected_projection || "cylindrical",
        panoramaMessage: "",
      });
    },
    [getState, update],
  );

  /** Toggle source inclusion without mutating the shared image-id array. */
  const togglePanoramaSource = useCallback(
    (imageId: number): void =>
      update((current) => ({
        panoramaImageIds: current.panoramaImageIds.includes(imageId)
          ? current.panoramaImageIds.filter((id) => id !== imageId)
          : [...current.panoramaImageIds, imageId],
        panoramaMessage: "",
      })),
    [update],
  );

  /** Reorder selected sources while keeping the neighboring-image swap behavior. */
  const movePanoramaSource = useCallback(
    (imageId: number, direction: number): void => {
      const ids = [...getState().panoramaImageIds];
      const index = ids.indexOf(imageId);
      const target = index + direction;
      if (index < 0 || target < 0 || target >= ids.length) return;
      const selected = ids[index];
      const neighbor = ids[target];
      if (selected === undefined || neighbor === undefined) return;
      [ids[index], ids[target]] = [neighbor, selected];
      update({ panoramaImageIds: ids });
    },
    [getState, update],
  );

  /** Merge user edits through context so controlled name and projection inputs rerender automatically. */
  const updatePanorama = useCallback((patch: PanoramaDraft): void => update(patch), [update]);

  /** Run one project operation and merge its returned snapshot before the next operation starts. */
  const request = async (operation: Promise<ReviewStateMessage>): Promise<void> => {
    session.applyMessage(await operation);
  };

  /** Create or update the project, then request the preview projections for its selected sources. */
  const generatePanoramaPreviews = async (): Promise<void> => {
    update({ panoramaMessage: "Starting previews" });
    try {
      const current = getState();
      const body = {
        image_ids: current.panoramaImageIds,
        name: current.panoramaName,
        matching_mode: current.panoramaMatching,
      };
      let projectId = current.panoramaProjectId;
      if (projectId === null) {
        const existing = new Set(current.data?.panorama.projects.map((project) => project.id) || []);
        await request(reviewApi.panorama_create({ body }));
        const created = getState()
          .data?.panorama.projects.filter((project) => !existing.has(project.id))
          .sort((left, right) => right.id - left.id)[0];
        if (!created) throw new Error("created panorama project was not returned");
        projectId = created.id;
        update({ panoramaProjectId: projectId });
      } else await request(reviewApi.panorama_update({ params: { project_id: projectId }, body }));
      await request(
        reviewApi.panorama_previews({
          params: { project_id: projectId },
          body: {
            image_ids: current.panoramaImageIds,
            matching_mode: current.panoramaMatching,
          },
        }),
      );
      update({ panoramaMessage: "" });
    } catch (error) {
      update({ panoramaMessage: `Preview failed: ${errorMessage(error)}` });
    }
  };

  /** Request the full panorama using the currently selected preview projection and output name. */
  const renderPanoramaFinal = async (): Promise<void> => {
    const current = getState();
    if (current.panoramaProjectId === null) return;
    update({ panoramaMessage: "Starting full render" });
    try {
      await request(
        reviewApi.panorama_render({
          params: { project_id: current.panoramaProjectId },
          body: {
            name: current.panoramaName,
            projection: current.panoramaProjection,
          },
        }),
      );
      update({ panoramaMessage: "" });
    } catch (error) {
      update({ panoramaMessage: `Render failed: ${errorMessage(error)}` });
    }
  };

  /** Read the current project from live server state so progress changes appear immediately. */
  const currentPanoramaProject = (): ReviewPanoramaProject | null =>
    state.data?.panorama.projects.find((project) => project.id === state.panoramaProjectId) || null;
  return {
    openPanoramaWizard,
    closePanoramaWizard,
    selectPanoramaProject,
    togglePanoramaSource,
    movePanoramaSource,
    updatePanorama,
    generatePanoramaPreviews,
    renderPanoramaFinal,
    currentPanoramaProject,
  };
}
