/** Publish waits for its fixed local invalidation set; unrelated unfinished renders keep their skip semantics. */
import type { PublishRequest, ReviewImageObservation, ReviewStateObservation } from "../../core/types";
import { imageLabels, isDirectCompressedImage, publishProfileIndexes } from "../../core/selectors";
import type { CommitContext } from "../../session/barriers";

/** Match daemon selection rules after captured edits are committed, including edits that change filter membership. */
function selected(image: ReviewImageObservation, request: PublishRequest): boolean {
  return (
    image.rating >= request.min_rating &&
    (!request.labels.length || imageLabels(image).some((label) => request.labels.includes(label))) &&
    (!request.tags.length || image.tags.some((tag) => request.tags.includes(tag.toLowerCase())))
  );
}

/** Capture immutable output identities rather than chasing later changes to filters, profiles, or local drafts. */
export function publishWaitSet(context: CommitContext, request: PublishRequest): readonly string[] {
  return (context.snapshot.data?.images || []).flatMap((image): string[] => {
    if (!selected(image, request)) return [];
    const profiles = request.main_profile_only
      ? context.snapshot.data?.profiles.slice(0, 1).map((profile) => profile.index) || []
      : publishProfileIndexes(image);
    const keys = isDirectCompressedImage(image)
      ? [`${image.id}:preview`]
      : profiles.map((profile) => `${image.id}:${profile}`);
    return keys.filter((key) => context.affectedOutputs.has(key));
  });
}

/** Return pending readiness or a terminal problem using the same public status/media fields as the daemon UI. */
export function publishOutputsReady(state: ReviewStateObservation, targets: readonly string[]): boolean {
  let ready = true;
  for (const target of targets) {
    const [imageKey, profileKey] = target.split(":");
    const image = state.data?.images.find((candidate) => candidate.id === Number(imageKey));
    if (!image) throw new Error(`Picture ${imageKey || ""} is no longer available`);
    if (profileKey === "preview") {
      if (image.preview_error || image.preview_status === "failed")
        throw new Error(image.preview_error || `Picture ${image.id} preview failed`);
      ready = ready && image.preview_status === "done" && !image.preview_retouch_pending && image.preview_url !== null;
    } else {
      const render = image.profiles.find((profile) => profile.profile_index === Number(profileKey));
      if (!render || !render.enabled) throw new Error(`Picture ${image.id} publish profile is no longer available`);
      if (render.error || render.status === "failed")
        throw new Error(render.error || `Picture ${image.id} render failed`);
      ready = ready && render.status === "done" && !render.retouch_pending && render.url !== null;
    }
  }
  return ready;
}

/** A finite wait owns its timer and aborts read-only I/O on expiry; it never retries a mutation. */
export async function waitForPublishOutputs(context: CommitContext, request: PublishRequest): Promise<void> {
  const targets = publishWaitSet(context, request);
  const controller = new AbortController();
  const timer = setTimeout((): void => controller.abort(), 120_000);
  try {
    while (true) {
      const snapshot = await context.refresh(controller.signal);
      if (publishOutputsReady(snapshot, targets)) return;
      await new Promise<void>((resolve) => setTimeout(resolve, 500));
      if (controller.signal.aborted) throw new Error("Timed out waiting for edited pictures to finish rendering");
    }
  } finally {
    clearTimeout(timer);
    controller.abort();
  }
}

/** The endpoint starts its child before SQLite is read; retain the local write fence until task selection. */
export async function waitForPublishSnapshot(context: CommitContext, jobId: number): Promise<void> {
  const controller = new AbortController();
  const timer = setTimeout((): void => controller.abort(), 30_000);
  try {
    while (true) {
      const snapshot = await context.refresh(controller.signal);
      const job = snapshot.data?.publish_jobs.find((candidate) => candidate.id === jobId);
      if (!job) throw new Error(`Publish job ${jobId} was not present in the state check`);
      if (job.status !== "running" || job.step !== "starting") return;
      await new Promise<void>((resolve) => setTimeout(resolve, 250));
      if (controller.signal.aborted) throw new Error("Publish startup could not be confirmed");
    }
  } finally {
    clearTimeout(timer);
    controller.abort();
  }
}
