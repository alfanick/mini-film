/** Share established editing limits and tool presets so all components use the same backend-compatible values. */
import type {
  BwFilter,
  ColorLabel,
  DiffusionMethod,
  DiffusionSettings,
  PanoramaMatching,
  PanoramaProjection,
} from "./types";

export const RETOUCH_SAVE_DEBOUNCE_MS = 1200;

export const RETOUCH_TEMPERATURE_DELTA_LIMIT = 2500;

export const RETOUCH_OFFSET_DELTA_LIMIT = 100;

export const HISTOGRAM_SAMPLE_LONG_EDGE = 512;

export const HISTOGRAM_RETOUCH_DEBOUNCE_MS = 100;

export const COMPRESSED_REVIEW_PREVIEW_LONG_EDGE = 2048;

export const TOUCH_SWIPE_MIN_PX = 72;

export const TOUCH_SWIPE_RATIO = 1.65;

export const ZOOM_LONG_PRESS_MS = 380;

export const ZOOM_MOVE_CANCEL_PX = 22;

export const ZOOM_LOUPE_TOUCH_GAP_PX = 28;

export const ZOOM_LOUPE_POINTER_GAP_PX = 18;

export const WHEEL_NAV_THRESHOLD_PX = 90;

export const WHEEL_NAV_RESET_MS = 220;

export const WHEEL_NAV_COOLDOWN_MS = 260;

export const RATING_VALUES = [0, 1, 2, 3, 4, 5];

export const COLOR_LABELS: ColorLabel[] = ["red", "yellow", "green", "blue", "purple"];

export const BW_FILTERS: BwFilter[] = ["none", "yellow", "orange", "red", "green"];

export const BW_FILTER_LABELS = new Map<BwFilter, string>([
  ["none", "None"],
  ["yellow", "Y"],
  ["orange", "O"],
  ["red", "R"],
  ["green", "G"],
]);

export const BW_FILTER_NAMES = new Map<BwFilter, string>([
  ["none", "No"],
  ["yellow", "Yellow"],
  ["orange", "Orange"],
  ["red", "Red"],
  ["green", "Green"],
]);

export const CROP_RATIO_PRESETS: [string, string, string?][] = [
  ["original", "Original"],
  ["free", "Free"],
  ["4:3", "4:3"],
  ["5:4", "5:4"],
  ["a3-a4", "A3/A4", "A3/A4 portrait"],
  ["1:1", "1:1"],
  ["16:10", "16:10"],
  ["21:9", "21:9"],
  ["3:1", "3:1"],
  ["4:1", "4:1"],
  ["5:1", "5:1"],
  ["6:1", "6:1"],
];

export const PANORAMA_MATCHING_MODES: [PanoramaMatching, string][] = [
  ["automatic", "Automatic"],
  ["sequential", "Sequential"],
  ["multi-row", "Multi-row"],
  ["flat-mosaic", "Flat mosaic"],
];

export const PANORAMA_PROJECTIONS: [PanoramaProjection, string][] = [
  ["rectilinear", "Rectilinear"],
  ["cylindrical", "Cylindrical"],
  ["equirectangular", "Equirectangular"],
  ["panini", "General Panini"],
];

export const SAMPLER_POLL_MS = 500;

export const SAMPLER_PRIORITY_DEBOUNCE_MS = 60;

export const DIFFUSION_POLL_MS = 500;

export const DIFFUSION_PREVIEW_DEBOUNCE_MS = 280;

export const DIFFUSION_METHODS: { id: DiffusionMethod; label: string; description: string }[] = [
  {
    id: "multi-scale-mist",
    label: "Multi-scale mist",
    description: "Layered optical diffusion with broad, natural highlight spread.",
  },
  {
    id: "edge-aware-glow",
    label: "Edge-aware glow",
    description: "Protects defined edges while blooming bright areas.",
  },
];

export const DIFFUSION_PRESETS: (Omit<DiffusionSettings, "method"> & {
  id: string;
  label: string;
  description: string;
})[] = [
  {
    id: "off",
    label: "Off",
    description: "No diffusion",
    softness: 0,
    highlight_glow: 0,
    softness_radius_percent: 100,
    glow_radius_percent: 100,
    intensity_percent: 100,
    highlight_reach: 50,
  },
  {
    id: "subtle",
    label: "Subtle",
    description: "Visible, gentle diffusion",
    softness: 25,
    highlight_glow: 25,
    softness_radius_percent: 100,
    glow_radius_percent: 150,
    intensity_percent: 150,
    highlight_reach: 50,
  },
  {
    id: "medium",
    label: "Medium",
    description: "Clear film-diffusion character",
    softness: 50,
    highlight_glow: 50,
    softness_radius_percent: 150,
    glow_radius_percent: 225,
    intensity_percent: 225,
    highlight_reach: 60,
  },
  {
    id: "strong",
    label: "Strong",
    description: "Bold softness and bloom",
    softness: 75,
    highlight_glow: 75,
    softness_radius_percent: 200,
    glow_radius_percent: 300,
    intensity_percent: 300,
    highlight_reach: 70,
  },
];

export const DIFFUSION_DETAIL_AREAS = [
  { kind: "focus", label: "Focus area" },
  { kind: "high-contrast-highlight", label: "High-contrast highlight" },
  { kind: "broad-highlight", label: "Broad highlight" },
];
