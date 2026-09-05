/** Reconstruct portable retouch deltas from the controls' effective profile values.
 * Reading any slider historically read every bounded input, so clipped siblings must normalize together. */
import { clamp, isDirectCompressedImage, normalizedRetouch } from "../core/selectors";
import type { RetouchSettings, ReviewImage, ReviewProfile } from "../core/types";

/** Match the seven tonal input bounds while preserving crop and camera-relative white balance. */
export function retouchFromVisibleControls(
  value: RetouchSettings,
  profile: ReviewProfile | null,
  image: ReviewImage | null,
): RetouchSettings {
  const retouch = normalizedRetouch(value);
  const base = normalizedRetouch(profile ? { adjustments: profile.retouch_base } : null).adjustments;
  for (const key of ["exposure", "contrast", "highlights", "shadows", "whites", "blacks", "clarity"] as const) {
    const limit = key === "exposure" ? 4 : 100;
    retouch.adjustments[key] = clamp(base[key] + retouch.adjustments[key], -limit, limit) - base[key];
  }
  if (isDirectCompressedImage(image)) retouch.adjustments = normalizedRetouch(null).adjustments;
  return normalizedRetouch(retouch);
}
