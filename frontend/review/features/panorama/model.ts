/** Provider-owned panorama actions serialize submissions and retain local drafts across asynchronous server replies. */
import { computed, createModel, effect, signal, type ReadonlySignal } from "@preact/signals";
import { reviewApi, errorMessage } from "../../core/api";
import type { ReviewModelValue } from "../../core/model";
import type {
  ReviewCatalogObservation,
  ReviewPanoramaProjectObservation,
  ReviewStateObservation,
  PanoramaMatching,
  PanoramaProjection,
} from "../../core/types";
import type { ToolSessionActions } from "../../tools/types";

/** User-editable project choices change independently of server progress. */
export interface PanoramaDraft {
  readonly panoramaName?: string;
  readonly panoramaMatching?: PanoramaMatching;
  readonly panoramaProjection?: PanoramaProjection;
}

/** Immutable project choices remain local and independent of confirmed server projects. */
interface PanoramaDraftState {
  readonly panoramaProjectId: number | null;
  readonly panoramaImageIds: readonly number[];
  readonly panoramaName: string;
  readonly panoramaMatching: PanoramaMatching;
  readonly panoramaProjection: PanoramaProjection;
  readonly panoramaMessage: string;
}

/** Only panorama controls and their source/project lists participate in this feature's subscription. */
export interface PanoramaState extends PanoramaDraftState {
  readonly panoramaOpen: boolean;
  readonly data: Pick<ReviewCatalogObservation, "images" | "panorama"> | null;
  readonly minRating: number;
}

/** Stable wizard commands preserve the raw-source workflow without depending on the review-save queue. */
export interface PanoramaActions {
  openPanoramaWizard(this: void): void;
  closePanoramaWizard(this: void): void;
  currentPanoramaProject(this: void): ReviewPanoramaProjectObservation | null;
  selectPanoramaProject(this: void, value: string): void;
  togglePanoramaSource(this: void, imageId: number): void;
  movePanoramaSource(this: void, imageId: number, direction: number): void;
  updatePanorama(this: void, patch: PanoramaDraft): void;
  generatePanoramaPreviews(this: void): Promise<void>;
  renderPanoramaFinal(this: void): Promise<void>;
}

/** A synchronous operation signal closes the render-frame gap between repeated submit events. */
export interface PanoramaModelValue extends PanoramaActions {
  readonly state: ReadonlySignal<PanoramaState>;
  readonly operation: ReadonlySignal<"idle" | "preview" | "render">;
  readonly updateSharedUi: ToolSessionActions["updateSharedUi"];
}

/** Choose adjacent source pictures and retain the existing initial name for a new project. */
function newDraft(state: ReviewStateObservation): PanoramaDraftState {
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

/** Keep draft identity, operation ownership and subscriptions independent of recoverable wizard views. */
export const PanoramaModel = createModel(
  (
    catalog: ReviewModelValue,
    session: Pick<ToolSessionActions, "applyMessage" | "updateSharedUi">,
  ): PanoramaModelValue => {
    const opened = signal(false);
    const draft = signal<PanoramaDraftState>({
      panoramaProjectId: null,
      panoramaImageIds: [],
      panoramaName: "Panorama",
      panoramaMatching: "automatic",
      panoramaProjection: "cylindrical",
      panoramaMessage: "",
    });
    const operation = signal<"idle" | "preview" | "render">("idle");
    let flight: Promise<void> | null = null;
    let generation = 0;
    let revision = 0;
    let disposed = false;
    const images = computed(() => catalog.catalog.value?.images ?? []);
    const panorama = computed(() => catalog.catalog.value?.panorama ?? null);
    const minRating = computed(() => catalog.catalog.value?.ui.min_rating ?? 0);
    const state = computed((): PanoramaState => {
      const panoramaOpen = opened.value;
      const projects = panoramaOpen ? panorama.value : null;
      return {
        panoramaOpen,
        ...draft.value,
        minRating: panoramaOpen ? minRating.value : 0,
        data: panoramaOpen && projects ? { images: images.value, panorama: projects } : null,
      };
    });

    /** Replace only this model's local draft; acknowledgements never receive direct draft write access. */
    function updateDraft(patch: Partial<PanoramaDraftState>): void {
      draft.value = { ...draft.peek(), ...patch };
    }

    /** Replace dialog identity so old replies cannot select a project inside a newer opening. */
    function openPanoramaWizard(): void {
      const current = catalog.getState();
      if (disposed || !current.data?.capabilities.panorama.available) return;
      generation += 1;
      revision += 1;
      updateDraft({
        ...(draft.peek().panoramaProjectId === null ? newDraft(current) : {}),
        panoramaMessage: "",
      });
      opened.value = true;
    }

    /** Hide the wizard and invalidate its continuation without aborting an already-submitted server mutation. */
    function closePanoramaWizard(): void {
      generation += 1;
      opened.value = false;
    }

    /** Select a saved project or initialize a distinct new-project draft. */
    function selectPanoramaProject(value: string): void {
      const current = catalog.getState();
      if (value === "new") {
        generation += 1;
        revision += 1;
        updateDraft(newDraft(current));
        return;
      }
      const project = current.data?.panorama.projects.find((candidate) => candidate.id === Number(value));
      if (!project) return;
      generation += 1;
      revision += 1;
      updateDraft({
        panoramaProjectId: project.id,
        panoramaImageIds: [...project.image_ids],
        panoramaName: project.name || "Panorama",
        panoramaMatching: project.matching_mode || "automatic",
        panoramaProjection: project.selected_projection || "cylindrical",
        panoramaMessage: "",
      });
    }

    /** Toggle source inclusion without mutating the observed image-id sequence. */
    function togglePanoramaSource(imageId: number): void {
      revision += 1;
      const current = draft.peek();
      updateDraft({
        panoramaImageIds: current.panoramaImageIds.includes(imageId)
          ? current.panoramaImageIds.filter((id) => id !== imageId)
          : [...current.panoramaImageIds, imageId],
        panoramaMessage: "",
      });
    }

    /** Swap selected neighbors while retaining user-defined panorama source order. */
    function movePanoramaSource(imageId: number, direction: number): void {
      const ids = [...draft.peek().panoramaImageIds];
      const index = ids.indexOf(imageId);
      const target = index + direction;
      if (index < 0 || target < 0 || target >= ids.length) return;
      const selected = ids[index];
      const neighbor = ids[target];
      if (selected === undefined || neighbor === undefined) return;
      [ids[index], ids[target]] = [neighbor, selected];
      revision += 1;
      updateDraft({ panoramaImageIds: ids, panoramaMessage: "" });
    }

    /** Record draft ownership without replacing it when a submitted snapshot is later acknowledged. */
    function updatePanorama(patch: PanoramaDraft): void {
      revision += 1;
      updateDraft({ ...patch, panoramaMessage: "" });
    }

    /** A response may update server progress, but only its current dialog generation may change local presentation. */
    function isCurrent(expectedGeneration: number): boolean {
      return !disposed && generation === expectedGeneration;
    }

    /** Create/update and preview exactly the submitted raw sources, using the server's explicit created identity. */
    async function preview(
      current: PanoramaDraftState,
      expectedGeneration: number,
      submittedRevision: number,
    ): Promise<void> {
      updateDraft({ panoramaMessage: "Starting previews" });
      try {
        const body = {
          image_ids: [...current.panoramaImageIds],
          name: current.panoramaName,
          matching_mode: current.panoramaMatching,
        };
        let projectId = current.panoramaProjectId;
        if (projectId === null) {
          const response = await reviewApi.panorama_create({ body });
          if (disposed) return;
          session.applyMessage(response);
          if (!isCurrent(expectedGeneration)) return;
          projectId = response.created_project_id;
          updateDraft({ panoramaProjectId: projectId });
        } else {
          const response = await reviewApi.panorama_update({ params: { project_id: projectId }, body });
          if (disposed) return;
          session.applyMessage(response);
          if (!isCurrent(expectedGeneration)) return;
        }
        const response = await reviewApi.panorama_previews({
          params: { project_id: projectId },
          body: { image_ids: [...current.panoramaImageIds], matching_mode: current.panoramaMatching },
        });
        if (disposed) return;
        session.applyMessage(response);
        if (isCurrent(expectedGeneration) && revision === submittedRevision) updateDraft({ panoramaMessage: "" });
      } catch (error: unknown) {
        if (isCurrent(expectedGeneration) && revision === submittedRevision)
          updateDraft({ panoramaMessage: `Preview failed: ${errorMessage(error)}` });
      }
    }

    /** Submit a full render while keeping newer local names and projections intact. */
    async function render(
      current: PanoramaDraftState,
      expectedGeneration: number,
      submittedRevision: number,
    ): Promise<void> {
      if (current.panoramaProjectId === null) return;
      updateDraft({ panoramaMessage: "Starting full render" });
      try {
        const response = await reviewApi.panorama_render({
          params: { project_id: current.panoramaProjectId },
          body: { name: current.panoramaName, projection: current.panoramaProjection },
        });
        if (disposed) return;
        session.applyMessage(response);
        if (isCurrent(expectedGeneration) && revision === submittedRevision) updateDraft({ panoramaMessage: "" });
      } catch (error: unknown) {
        if (isCurrent(expectedGeneration) && revision === submittedRevision)
          updateDraft({ panoramaMessage: `Render failed: ${errorMessage(error)}` });
      }
    }

    /** Set the synchronous guard before starting transport so repeated events share one operation. */
    function submit(kind: "preview" | "render"): Promise<void> {
      if (flight) return flight;
      const current = state.peek();
      if (disposed || !current.panoramaOpen || (kind === "render" && current.panoramaProjectId === null))
        return Promise.resolve();
      operation.value = kind;
      const task = (kind === "preview" ? preview : render)(current, generation, revision).finally((): void => {
        if (flight === task) {
          flight = null;
          operation.value = "idle";
        }
      });
      flight = task;
      return task;
    }

    /** Request all preview projections from the current source draft. */
    function generatePanoramaPreviews(): Promise<void> {
      return submit("preview");
    }

    /** Request the full panorama from the selected preview projection. */
    function renderPanoramaFinal(): Promise<void> {
      return submit("render");
    }

    /** Read project progress at action time without retaining an old catalog snapshot. */
    function currentPanoramaProject(): ReviewPanoramaProjectObservation | null {
      const projectId = draft.peek().panoramaProjectId;
      return panorama.peek()?.projects.find((project) => project.id === projectId) ?? null;
    }

    /** End result ownership only when the application provider disposes the model. */
    effect(() => (): void => {
      disposed = true;
      generation += 1;
    });

    return {
      state,
      operation,
      updateSharedUi: session.updateSharedUi,
      openPanoramaWizard,
      closePanoramaWizard,
      currentPanoramaProject,
      selectPanoramaProject,
      togglePanoramaSource,
      movePanoramaSource,
      updatePanorama,
      generatePanoramaPreviews,
      renderPanoramaFinal,
    };
  },
);
