/**
 * Publish form conversion and output comparison preserve the existing export request contract without reading DOM
 * fields.
 */
import type { ReviewPublishJob } from "../core/types";

/** Clamp progress to an honest percentage and recognize completed empty jobs. */
export function publishProgressPercent(job: ReviewPublishJob | null): number {
  const total = Number(job?.total || 0);
  const processed = Number(job?.processed || 0);
  if (total <= 0) return job?.status === "done" ? 100 : 0;
  return Math.max(0, Math.min(100, Math.round((processed / total) * 100)));
}

/** Convert the publish tag field into the existing comma-or-whitespace token list. */
export function splitPublishTags(raw: string): string[] {
  return raw
    .split(/[,\s]+/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

/** Represent an absent or invalid positive dimension as JSON null. */
export function numberOrNull(value: string): number | null {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : null;
}
