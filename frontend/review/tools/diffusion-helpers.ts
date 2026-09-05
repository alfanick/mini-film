/**
 * Diffusion value and geometry helpers retain engine limits and preview framing while reactive hooks own requests and
 * state.
 */
import type { JSX } from "preact";
import type {
  DiffusionSettings,
  DiffusionMethod,
  DiffusionJob,
  DiffusionDetailArea,
  DiffusionPreviewContext,
  ImageSource,
  ReviewImage,
  ReviewProfileRender,
} from "../core/types";
import { DIFFUSION_METHODS, DIFFUSION_DETAIL_AREAS } from "../core/constants";
import { clamp } from "./common";

/** Present slider strengths with the same percentage suffix as the original UI. */
export function formatPercent(value: number): string {
  return `${value}%`;
}

/** Normalize all diffusion controls together so previews and saved settings match. */
export function normalizeDiffusionSettings(settings?: Partial<DiffusionSettings> | null): DiffusionSettings {
  const method =
    DIFFUSION_METHODS.find((candidate) => candidate.id === settings?.method)?.id ?? DIFFUSION_METHODS[0].id;
  return {
    method,
    softness: normalizeDiffusionAmount(settings?.softness, 0, 100, 0),
    highlight_glow: normalizeDiffusionAmount(settings?.highlight_glow, 0, 100, 0),
    softness_radius_percent: normalizeDiffusionAmount(settings?.softness_radius_percent, 50, 400, 100),
    glow_radius_percent: normalizeDiffusionAmount(settings?.glow_radius_percent, 50, 400, 100),
    intensity_percent: normalizeDiffusionAmount(settings?.intensity_percent, 25, 300, 100),
    highlight_reach: normalizeDiffusionAmount(settings?.highlight_reach, 0, 100, 50),
  };
}

/** Clamp and round a diffusion control, preserving the legacy fallback for nonfinite inputs. */
export function normalizeDiffusionAmount(
  value: number | null | undefined,
  min: number,
  max: number,
  fallback: number,
): number {
  const number = Number(value);
  return Number.isFinite(number) ? Math.round(clamp(number, min, max)) : fallback;
}

/** Identify equivalent settings to debounce duplicate preview requests. */
export function diffusionSettingsSignature(settings: Partial<DiffusionSettings> | null): string {
  const normalized = normalizeDiffusionSettings(settings);
  return [
    normalized.method,
    normalized.softness,
    normalized.highlight_glow,
    normalized.softness_radius_percent,
    normalized.glow_radius_percent,
    normalized.intensity_percent,
    normalized.highlight_reach,
  ].join(":");
}

/** A named strength combination applied without changing the user's chosen diffusion algorithm. */
export interface DiffusionPreset extends Omit<DiffusionSettings, "method"> {
  id: string;
  label: string;
  description: string;
}

/** Map a preset to controls while respecting method-specific highlight reach. */
export function diffusionPresetSettings(
  preset: DiffusionPreset,
  method: DiffusionMethod,
): Omit<DiffusionSettings, "method"> {
  return {
    softness: preset.softness,
    highlight_glow: preset.highlight_glow,
    softness_radius_percent: preset.softness_radius_percent,
    glow_radius_percent: preset.glow_radius_percent,
    intensity_percent: preset.intensity_percent,
    highlight_reach: method === "edge-aware-glow" ? preset.highlight_reach : 50,
  };
}

/** Mark a preset only when every represented control matches the current settings. */
export function diffusionPresetIsActive(preset: DiffusionPreset, settings: DiffusionSettings): boolean {
  if (preset.id === "off") return settings.softness === 0 && settings.highlight_glow === 0;
  const expected = diffusionPresetSettings(preset, settings.method);
  return Object.entries(expected).every(([key, value]) => settings[key as keyof typeof expected] === value);
}

/** Locate the render variant the diffusion dialog was opened for. */
export function diffusionProfile(image: ReviewImage | null, profileIndex: number | null): ReviewProfileRender | null {
  return (image?.profiles || []).find((profile) => profile.profile_index === profileIndex) || null;
}

/** Explain whether diffusion comes from the picture, profile, or daemon default. */
export function diffusionSourceLabel(source: string | null | undefined): string {
  const normalized = String(source || "")
    .trim()
    .toLowerCase();
  if (!normalized) return "Default: off";
  if (["current", "image", "picture"].includes(normalized)) return "Current picture override";
  if (["all", "profile", "global"].includes(normalized)) return "All-picture profile setting";
  if (normalized === "daemon") return "Daemon default";
  if (["default", "none", "off"].includes(normalized)) return "Default: off";
  return normalized.replace(/[_-]+/g, " ");
}

/** Accept the existing preview response aliases for the completed diffusion image. */
export function diffusionAfterSource(job: DiffusionJob | null): ImageSource {
  return {
    url: job?.after_url || job?.preview_url || job?.result_url || null,
    updatedAt: job?.after_updated_at || job?.updated_at,
  };
}

/** Clamp preview crops to available pixels so detail comparison never references an invalid rectangle. */
export function normalizeDiffusionDetailArea(
  area: DiffusionDetailArea | null,
  previewWidth: number,
  previewHeight: number,
): DiffusionDetailArea | null {
  if (!area || !DIFFUSION_DETAIL_AREAS.some((definition) => definition.kind === area.kind)) return null;
  const rawX = Number(area.x);
  const rawY = Number(area.y);
  const rawWidth = Number(area.width);
  const rawHeight = Number(area.height);
  if (![rawX, rawY, rawWidth, rawHeight].every(Number.isFinite) || rawWidth <= 0 || rawHeight <= 0) return null;
  const x = clamp(Math.round(rawX), 0, previewWidth - 1);
  const y = clamp(Math.round(rawY), 0, previewHeight - 1);
  const width = clamp(Math.round(rawWidth), 1, previewWidth - x);
  const height = clamp(Math.round(rawHeight), 1, previewHeight - y);
  return { kind: area.kind, x, y, width, height };
}

/** Reserve the selected detail area's aspect ratio while media loads. */
export function diffusionDetailFrameStyle(area: DiffusionDetailArea | null): JSX.CSSProperties {
  return area ? { aspectRatio: `${area.width} / ${area.height}` } : { aspectRatio: "1 / 1" };
}

/** Position a full preview behind the detail frame using normalized crop percentages. */
export function diffusionDetailMediaStyle(
  area: DiffusionDetailArea,
  previewContext: DiffusionPreviewContext,
): JSX.CSSProperties {
  return {
    width: `${((previewContext.width / area.width) * 100).toFixed(6)}%`,
    height: `${((previewContext.height / area.height) * 100).toFixed(6)}%`,
    left: `${((-area.x / area.width) * 100).toFixed(6)}%`,
    top: `${((-area.y / area.height) * 100).toFixed(6)}%`,
  };
}

/** Stop polling when a preview job completes, fails, or is cancelled. */
export function diffusionJobIsTerminal(job: DiffusionJob | null): boolean {
  return ["done", "failed", "cancelled"].includes(job?.status ?? "");
}
