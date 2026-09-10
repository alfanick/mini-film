/** Provider-owned information state keeps PP3 requests alive across view recovery and cancels obsolete detail loads. */
import { batch, computed, createModel, effect, signal, type ReadonlySignal } from "@preact/signals";
import { reviewUrl, errorMessage } from "../../core/api";
import type { ReviewModelValue } from "../../core/model";
import type {
  ReviewCatalogObservation,
  ReviewImageObservation,
  ReviewProfileObservation,
  ReviewProfileRenderObservation,
  ProfileInfoPp3,
  ReadonlyData,
} from "../../core/types";
import { profilePp3Key, profilePp3Url, profileRenderIndex } from "./helpers";

/** Only information-dialog fields participate in its render subscription. */
export interface InformationState {
  readonly currentId: number | null;
  readonly profileInfoProfileIndex: number | null;
  readonly profileInfoPp3: ReadonlyData<ProfileInfoPp3>;
  readonly commandInvocationOpen: boolean;
  readonly data: Pick<ReviewCatalogObservation, "profiles" | "invocation" | "version"> | null;
  readonly image: ReviewImageObservation | null;
}

/** A single dialog identity makes mutually exclusive overlays impossible to open together. */
type InformationDialog = { kind: "closed" } | { kind: "profile"; index: number } | { kind: "invocation" };

/** Stable actions never grant their callers permission to mutate configured metadata. */
export interface InformationActions {
  openProfileInfo(this: void, profile: ReviewProfileObservation | ReviewProfileRenderObservation): void;
  closeProfileInfo(this: void): void;
  openCommandInvocation(this: void): void;
  closeCommandInvocation(this: void): void;
  loadProfilePp3(this: void, image: ReviewImageObservation, profile: ReviewProfileObservation): Promise<void>;
  profileByIndex(this: void, index: number | null): ReviewProfileObservation | null;
}

/** A feature model exposes one narrow observation alongside stable commands. */
export interface InformationModelValue extends InformationActions {
  readonly state: ReadonlySignal<InformationState>;
}

const EMPTY_PROFILES: readonly ReviewProfileObservation[] = [];

/** Keep profile and invocation overlays mutually exclusive without subscribing closed views to catalog changes. */
export const InformationModel = createModel((catalog: ReviewModelValue): InformationModelValue => {
  const dialog = signal<InformationDialog>({ kind: "closed" });
  const pp3 = signal<ReadonlyData<ProfileInfoPp3>>({ status: "idle", key: null });
  let pending: AbortController | null = null;
  let disposed = false;
  const profiles = computed(() => catalog.catalog.value?.profiles ?? EMPTY_PROFILES);
  const invocation = computed(() => catalog.catalog.value?.invocation ?? null);
  const version = computed(() => catalog.catalog.value?.version ?? "");
  const state = computed((): InformationState => {
    const activeDialog = dialog.value;
    const profileInfoProfileIndex = activeDialog.kind === "profile" ? activeDialog.index : null;
    const commandInvocationOpen = activeDialog.kind === "invocation";
    const active = profileInfoProfileIndex !== null || commandInvocationOpen;
    const currentId = profileInfoProfileIndex !== null ? catalog.field("currentId").value : null;
    return {
      profileInfoProfileIndex,
      commandInvocationOpen,
      profileInfoPp3: pp3.value,
      currentId,
      image: currentId !== null ? catalog.image(currentId).value : null,
      data: active
        ? {
            profiles: profileInfoProfileIndex !== null ? profiles.value : EMPTY_PROFILES,
            invocation: commandInvocationOpen ? invocation.value : null,
            version: profileInfoProfileIndex !== null ? version.value : "",
          }
        : null,
    };
  });

  /** Clear request ownership before aborting so even an already-resolved fetch cannot publish stale text. */
  function clearPp3(): void {
    const controller = pending;
    pending = null;
    controller?.abort();
    pp3.value = { status: "idle", key: null };
  }

  /** Open the configured profile corresponding to a rendered variant. */
  function openProfileInfo(profile: ReviewProfileObservation | ReviewProfileRenderObservation): void {
    if (disposed) return;
    const index = profileRenderIndex(profile);
    batch((): void => {
      clearPp3();
      dialog.value = index === null ? { kind: "closed" } : { kind: "profile", index };
    });
  }

  /** Closing invalidates any detail result that can no longer be displayed. */
  function closeProfileInfo(): void {
    if (dialog.peek().kind !== "profile") return;
    batch((): void => {
      clearPp3();
      dialog.value = { kind: "closed" };
    });
  }

  /** Open the recorded invocation after closing its mutually exclusive profile dialog. */
  function openCommandInvocation(): void {
    if (disposed) return;
    batch((): void => {
      clearPp3();
      dialog.value = { kind: "invocation" };
    });
  }

  /** Hide the recorded command through the same action used by keyboard and pointer controls. */
  function closeCommandInvocation(): void {
    if (dialog.peek().kind === "invocation") dialog.value = { kind: "closed" };
  }

  /** Fetch PP3 only when requested, preserving exact text and rejecting obsolete response ownership. */
  async function loadProfilePp3(image: ReviewImageObservation, profile: ReviewProfileObservation): Promise<void> {
    const current = dialog.peek();
    if (disposed || current.kind !== "profile" || current.index !== profile.index) return;
    const key = profilePp3Key(image, profile);
    if (pp3.peek().key === key) return;
    pending?.abort();
    const controller = new AbortController();
    pending = controller;
    pp3.value = { status: "loading", key };
    try {
      const response = await fetch(reviewUrl(profilePp3Url(image, profile)), {
        cache: "no-store",
        signal: controller.signal,
      });
      const body = await response.text();
      if (!response.ok) {
        let message = `PP3 ${response.status}`;
        try {
          const failure: unknown = JSON.parse(body);
          if (failure && typeof failure === "object" && "error" in failure && typeof failure.error === "string")
            message = failure.error || message;
        } catch {
          if (body.trim()) message = body.trim();
        }
        throw new Error(message);
      }
      if (!disposed && pending === controller && !controller.signal.aborted)
        pp3.value = { status: "ready", key, text: body };
    } catch (error: unknown) {
      if (!disposed && pending === controller && !controller.signal.aborted)
        pp3.value = { status: "failed", key, error: `Could not load PP3: ${errorMessage(error)}` };
    } finally {
      if (pending === controller) pending = null;
    }
  }

  /** Cancel owned transport only when the application provider, not a recoverable view, is disposed. */
  effect(() => (): void => {
    disposed = true;
    pending?.abort();
    pending = null;
  });

  /** A removed configured profile cannot leave a dialog pointing at unavailable metadata. */
  effect((): void => {
    const current = dialog.value;
    if (current.kind === "profile" && !profiles.value.some((profile) => profile.index === current.index))
      closeProfileInfo();
  });

  /** Resolve profile metadata at action time without capturing an old catalog. */
  function profileByIndex(index: number | null): ReviewProfileObservation | null {
    return profiles.peek().find((profile) => profile.index === index) ?? null;
  }

  return {
    state,
    openProfileInfo,
    closeProfileInfo,
    openCommandInvocation,
    closeCommandInvocation,
    loadProfilePp3,
    profileByIndex,
  };
});
