/** Pure presentation helpers for photo lists, camera details, and profile downloads.
 * Keeping formatting separate from components makes sidebar text stable across reactive state updates. */
import type * as T from "../core/types";
import {
  isDirectCompressedImage,
  profileDisplayState,
  publishProfileIndexes,
  safeDownloadPart,
} from "../core/selectors";

export interface ProgressState {
  state: string;
  text: string;
  title: string;
}
export interface CaptureDisplay {
  day: string | null;
  text: string;
}
export interface ExifPart {
  text: string;
  title?: string;
  className?: string;
}

/** Supply declarative camera-detail spans with the original hover text and ordering. */
export function formatImageExif(image: T.ReviewImage | null): ExifPart[] {
  if (!image) return [];
  const exif = image.exif;
  const shutterDetails = [
    exif.shutter_mode ? `Shutter mode: ${exif.shutter_mode}` : "",
    exif.silent_photography ? "Silent photography: On" : "",
  ]
    .filter(Boolean)
    .join(" · ");
  const parts: ExifPart[] = [
    {
      text: exif.shooting_mode ? `Mode ${exif.shooting_mode}` : "",
      title: exif.release_mode ? `Release mode: ${exif.release_mode}` : "",
    },
    {
      text: exif.camera_model || "",
      className: "image-exif-camera",
      title: exif.shutter_count === null ? "" : `Shutter count: ${exif.shutter_count}`,
    },
    { text: formatExifFocalLength(exif.focal_length), title: exif.lens_model ? `Lens: ${exif.lens_model}` : "" },
    {
      text: exif.iso ? `ISO ${exif.iso}` : "",
      title: exif.auto_iso ? (exif.iso_auto_hi_limit ? `Auto ISO <= ${exif.iso_auto_hi_limit}` : "Auto ISO") : "",
    },
    { text: formatExifAperture(exif.aperture) },
    { text: exif.shutter_speed || "", title: shutterDetails },
    { text: formatExposureCompensation(exif.exposure_compensation) },
    { text: exif.flash ? `Flash ${exif.flash}` : "" },
  ];
  const visible = parts.filter((part) => part.text);
  const summary = visible.map((part) => part.text).join(" · ");
  return visible.map((part) => ({ ...part, title: part.title || summary }));
}

/** Format a capture timestamp in local review time without changing the camera-provided value. */
export function captureTimeDisplay(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "";
  const date = new Date(value * 1000);
  if (!Number.isFinite(date.getTime())) return "";
  const day = `${date.getFullYear()}-${zeroPad(date.getMonth() + 1)}-${zeroPad(date.getDate())}`;
  const time = `${zeroPad(date.getHours())}:${zeroPad(date.getMinutes())}:${zeroPad(date.getSeconds())}`;
  return `${day} ${time}`;
}

/** Show precise elapsed capture time within a camera-provided burst. */
export function burstCaptureDeltaDisplay(firstImage: T.ReviewImage, image: T.ReviewImage): string {
  const firstCapture = preciseCaptureTime(firstImage);
  const capture = preciseCaptureTime(image);
  if (!firstCapture || !capture) return "";

  const delta = capture.timestamp - firstCapture.timestamp + capture.subsecond - firstCapture.subsecond;
  return Number.isFinite(delta) && delta >= 0 ? `+${delta.toFixed(2)}s` : "";
}

/** Read capture subseconds without rounding away burst ordering. */
export function preciseCaptureTime(image: T.ReviewImage): { timestamp: number; subsecond: number } | null {
  const rawTimestamp = image?.exif?.capture_timestamp;
  const timestamp = Number(rawTimestamp);
  const subsecond = image?.exif?.capture_subsecond;
  if (
    rawTimestamp === null ||
    rawTimestamp === undefined ||
    !Number.isSafeInteger(timestamp) ||
    typeof subsecond !== "string" ||
    !/^\d+$/.test(subsecond)
  ) {
    return null;
  }

  const fraction = Number(`0.${subsecond}`);
  return Number.isFinite(fraction) ? { timestamp, subsecond: fraction } : null;
}

/** Keep keyboard activation of burst buttons from also rating the picture. */
export function isolateBurstActivation(event: MouseEvent | KeyboardEvent): void {
  if ("key" in event && (event.key === "Enter" || event.key === " ")) event.stopPropagation();
}

/** Hide only the final image extension in the compact picture list. */
export function sidebarFileStem(fileName: string): string {
  const name = typeof fileName === "string" ? fileName : "";
  const extension = name.lastIndexOf(".");
  return extension > 0 ? name.slice(0, extension) : name;
}

/** Shorten known camera vendor prefixes and Nikon Z generation names. */
export function sidebarCameraModel(cameraModel: string | null | undefined): string {
  const original = typeof cameraModel === "string" ? cameraModel.trim() : "";
  if (!original) return "";

  const nikon = /^(?:nikon(?:\s+corporation)?\s+)?z\s*(fc|f|\d+)(?:[\s_-]*(?:mark\s*)?(\d+|ii|iii|iv))?$/i.exec(
    original,
  );
  if (nikon) {
    const generations: Record<string, string> = { 2: "ii", 3: "iii", 4: "iv", ii: "ii", iii: "iii", iv: "iv" };
    const generation = generations[(nikon[2] || "").toLowerCase()];
    const body = /^\d+$/.test(nikon[1]) ? nikon[1] : nikon[1].toLowerCase();
    return `Z${body}${generation || ""}`;
  }

  return original
    .replace(
      new RegExp(
        [
          String.raw`^(?:nikon(?:\s+corporation)?`,
          String.raw`canon(?:\s+inc\.?)?`,
          String.raw`sony(?:\s+corporation)?`,
          String.raw`fujifilm(?:\s+corporation)?`,
          String.raw`fuji`,
          String.raw`olympus(?:\s+imaging(?:\s+corp(?:oration)?\.?)?)?`,
          String.raw`om\s+(?:system`,
          String.raw`digital solutions)(?:\s+corporation)?`,
          String.raw`panasonic(?:\s+corporation)?`,
          String.raw`lumix`,
          String.raw`leica(?:\s+camera(?:\s+ag)?)?`,
          String.raw`pentax`,
          String.raw`ricoh(?:\s+imaging(?:\s+company)?(?:,?\s+ltd\.?)?)?`,
          String.raw`hasselblad`,
          String.raw`sigma`,
          String.raw`samsung`,
          String.raw`apple`,
          String.raw`google`,
          String.raw`dji`,
          String.raw`phase one`,
          String.raw`kodak)(?=$`,
          String.raw`[\s_:/-])[\s_:/-]*`,
        ].join("|"),
        "i",
      ),
      "",
    )
    .trim();
}

/** Summarize render and analysis progress for one picture without reading global state. */
export function renderProgressSummary(
  image: T.ReviewImage,
  localDirty: boolean = false,
  implicitProfiles: boolean = false,
): ProgressState {
  if (isDirectCompressedImage(image)) {
    if (localDirty) {
      return {
        state: "retouch-draft",
        text: "crop draft",
        title: "crop draft preview is local; server render will queue after edits settle",
      };
    }
    const codexState = codexProgressState(image);
    if (codexState) return codexState;
    const display = profileDisplayState(image, null);
    if (display.state === "done") {
      return {
        state: "ready",
        text: "ready",
        title: "image ready",
      };
    }
    return display;
  }

  const publishIndexes = new Set(publishProfileIndexes(image));
  const profiles = (image.profiles || []).filter((profile) => publishIndexes.has(profile.profile_index));
  const total = profiles.length;
  const done = profiles.filter((profile) => profile.status === "done").length;
  const failed = profiles.filter((profile) => profile.status === "failed").length;
  const retouchProcessing = profiles.some((profile) => profile.retouch_pending && profile.status === "processing");
  const retouchQueued = profiles.some((profile) => profile.retouch_pending && profile.status === "queued");
  const processing = profiles.some((profile) => profile.status === "processing");
  const queued = profiles.some((profile) => profile.status === "queued");
  const previewReady = Boolean(image.preview_url);

  if (localDirty) {
    return {
      state: "retouch-draft",
      text: "retouch draft",
      title: "retouch draft preview is local; server render will queue after edits settle",
    };
  }
  const codexState = codexProgressState(image);
  if (codexState) return codexState;
  if (total === 0) {
    return {
      state: "waiting",
      text: "none",
      title: implicitProfiles ? "RawTherapee default render pending" : "no profiles selected for publish",
    };
  }
  if (failed > 0 && done + failed === total) {
    return {
      state: "failed",
      text: `${done}/${total}`,
      title: `${done} profiles ready, ${failed} failed`,
    };
  }
  if (total > 0 && done === total) {
    return {
      state: "ready",
      text: "ready",
      title: "all profiles are ready",
    };
  }
  if (retouchProcessing) {
    return {
      state: "retouch-processing",
      text: `retouch ${done}/${total}`,
      title: `${done} of ${total} profiles ready, retouch render running`,
    };
  }
  if (retouchQueued) {
    return {
      state: "retouch-queued",
      text: `retouch ${done}/${total}`,
      title: `${done} of ${total} profiles ready, retouch render queued`,
    };
  }
  if (processing) {
    return {
      state: "processing",
      text: `${done}/${total}`,
      title: `${done} of ${total} profiles ready, processing`,
    };
  }
  if (queued) {
    return {
      state: "queued",
      text: `${done}/${total}`,
      title: `${done} of ${total} profiles ready, queued`,
    };
  }
  if (previewReady) {
    return {
      state: "preview",
      text: "preview",
      title: "camera preview ready, profiles pending",
    };
  }
  return {
    state: "waiting",
    text: "waiting",
    title: "waiting for preview and profiles",
  };
}

/** Give optional analysis work the same queued, running, and failed status vocabulary. */
export function codexProgressState(image: T.ReviewImage | null): ProgressState | null {
  const status = image?.codex?.status;
  if (status === "processing") {
    return {
      state: "processing",
      text: "codex",
      title: "Codex image analysis is running",
    };
  }
  if (status === "queued") {
    return {
      state: "queued",
      text: "codex",
      title: "Codex image analysis is queued",
    };
  }
  if (status === "failed") {
    return {
      state: "failed",
      text: "codex",
      title: `Codex image analysis failed${image?.codex.error ? `: ${image.codex.error}` : ""}`,
    };
  }
  return null;
}

/** Show the date for the first picture of a day and the time for later pictures. */
export function imageCaptureDisplay(image: T.ReviewImage | null, previousDay: string | null): CaptureDisplay {
  const timestamp = Number(image?.exif?.capture_timestamp || NaN);
  if (!Number.isFinite(timestamp)) {
    return { day: previousDay, text: "" };
  }

  const date = new Date(timestamp * 1000);
  if (!Number.isFinite(date.getTime())) {
    return { day: previousDay, text: "" };
  }

  const day = `${date.getFullYear()}-${zeroPad(date.getMonth() + 1)}-${zeroPad(date.getDate())}`;
  const time = `${zeroPad(date.getHours())}:${zeroPad(date.getMinutes())}:${zeroPad(date.getSeconds())}`;
  const isFirstOfDay = day !== previousDay;
  return {
    day,
    text: isFirstOfDay ? `${day} ${time}` : time,
  };
}

/** Keep date and time fields a consistent two characters wide. */
export function zeroPad(value: number): string {
  return String(value).padStart(2, "0");
}

/** Show nonzero exposure compensation in the established EV notation. */
export function formatExposureCompensation(value: string | number | null | undefined): string {
  if (!value && value !== 0) return "";
  const number = Number(value);
  if (!Number.isFinite(number) || number === 0) return "";
  const normalized = Number(number.toFixed(1));
  if (normalized === 0) return "";
  return `${normalized > 0 ? "+" : ""}${normalized.toFixed(1)}EV`;
}

/** Round focal length metadata without removing its unit. */
export function formatExifFocalLength(value: string | number | null | undefined): string {
  return formatExifNumberText(value, 2);
}

/** Round aperture metadata while keeping its original notation. */
export function formatExifAperture(value: string | number | null | undefined): string {
  return formatExifNumberText(value, 1);
}

/** Round numbers embedded in camera-provided text deterministically. */
export function formatExifNumberText(value: string | number | null | undefined, maxDigits: number): string {
  if (!value) return "";
  return String(value).replace(/[-+]?\d+(?:\.\d+)?/g, (match) => {
    const number = Number(match);
    if (!Number.isFinite(number)) return match;
    return number.toLocaleString("en-US", {
      maximumFractionDigits: maxDigits,
      useGrouping: false,
    });
  });
}

/** Prefer dimensions from the finished render when determining rail orientation. */
export function isPortraitRenderProfile(profile: T.ReviewProfileRender | null): boolean {
  const width = Number(profile?.width);
  const height = Number(profile?.height);
  return Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0 && height > width;
}

/** Describe original image size and pixel dimensions in the filename tooltip. */
export function imageSourceInfoTitle(image: T.ReviewImage | null): string {
  const parts = [];
  const fileSize = formatFileSize(image?.source_file_size_bytes);
  if (fileSize) parts.push(fileSize);

  const width = Number(image?.source_width);
  const height = Number(image?.source_height);
  if (Number.isFinite(width) && width > 0 && Number.isFinite(height) && height > 0) {
    const roundedWidth = Math.round(width);
    const roundedHeight = Math.round(height);
    parts.push(`${roundedWidth} x ${roundedHeight} px`);
    parts.push(`${((roundedWidth * roundedHeight) / 1_000_000).toFixed(1)} MP`);
  }

  return parts.join(" | ");
}

/** Format a known byte count with the same decimal units used by the existing review. */
export function formatFileSize(bytes: string | number | null | undefined): string {
  if (bytes === null || bytes === undefined || bytes === "") return "";
  let value = Number(bytes);
  if (!Number.isFinite(value) || value < 0) return "";
  if (value < 1000) return `${Math.round(value)} B`;

  const units = ["KB", "MB", "GB", "TB"];
  let unit = units[0];
  for (const candidate of units) {
    value /= 1000;
    unit = candidate;
    if (value < 1000) break;
  }
  return `${value.toFixed(1)} ${unit}`;
}

/** Generate a recognizable attachment filename from the source and selected profile. */
export function profileDownloadName(
  image: T.ReviewImage,
  profile: T.ReviewProfileRender & { selector?: string },
): string {
  const rawName = image.file_name || image.relative_path || "mini-film";
  const baseName = rawName.replace(/\.[^.]*$/, "");
  const profileName = profile.profile_stem || profile.selector || "profile";
  return `${safeDownloadPart(baseName)}--${safeDownloadPart(profileName)}.jpg`;
}

/** Include the rendered file size when it is known. */
export function profileDownloadTitle(profile: T.ReviewProfileRender, displayName: string): string {
  const rawBytes = profile.file_size_bytes;
  const bytes = rawBytes === null || rawBytes === undefined ? Number.NaN : Number(rawBytes);
  const size = Number.isFinite(bytes) && bytes >= 0 ? `${(bytes / 1_000_000).toFixed(1)} MB` : "";
  return size ? `Download rendered ${displayName} (${size})` : `Download rendered ${displayName}`;
}
