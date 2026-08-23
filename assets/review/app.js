import { Fragment, h, render as preactRender } from "./vendor/preact.module.js";

const state = {
  data: null,
  currentId: null,
  lastInputImageId: null,
  saveQueue: Promise.resolve(),
  preloaded: new Set(),
  viewerSafeAreaObserver: null,
  cropEditing: false,
  cropDraft: null,
  cropDraftRotation: 0,
  cropDraftImageId: null,
  cropSaved: null,
  cropSourceReady: false,
  cropGeometryInitialized: false,
  cropLayout: null,
  cropRatioKey: "original",
  cropRatioBase: null,
  cropRatioRotated: false,
  cropRatioGeometry: null,
  cropDrag: null,
  cropPointers: new Map(),
  cropTouchGesture: null,
  touchGesture: null,
  zoomPress: null,
  zoomActive: false,
  zoomPointerId: null,
  zoomLastPoint: null,
  zoomFullActive: false,
  zoomFullLastPoint: null,
  zoomSourceImage: null,
  zoomSourceUrl: null,
  gestureFeedbackTimer: null,
  retouchInputImageId: null,
  retouchClipboard: null,
  localRetouchDirty: false,
  retouchActiveSliderId: null,
  retouchActiveSliderOriginalValue: null,
  mobileDrawer: null,
  pendingProfileSelections: new Map(),
  profileInfoProfileIndex: null,
  profileInfoPp3: {
    key: null,
    text: null,
    error: null,
    loading: false,
    requestId: 0,
  },
  commandInvocationOpen: false,
  histogramOpen: false,
  histogramRequestId: 0,
  histogramTimer: null,
  informationOpen: false,
  panoramaOpen: false,
  panoramaProjectId: null,
  panoramaImageIds: [],
  panoramaName: "Panorama",
  panoramaMatching: "automatic",
  panoramaProjection: "cylindrical",
  panoramaMessage: "",
  samplerOpen: false,
  samplerLoading: false,
  samplerError: "",
  samplerJob: null,
  samplerExpandedSections: new Set(),
  samplerKnownEnabledKeys: new Set(),
  samplerVisibleKeys: new Set(),
  samplerSelectedKey: null,
  samplerPendingSelections: new Set(),
  samplerPollTimer: null,
  samplerPriorityTimer: null,
  samplerPrioritySignature: "",
  samplerPriorityController: null,
  samplerObserver: null,
  diffusionOpen: false,
  diffusionLoading: false,
  diffusionSaving: false,
  diffusionError: "",
  diffusionErrorKind: null,
  diffusionMessage: "",
  diffusionJob: null,
  diffusionBefore: null,
  diffusionPreviewContext: null,
  diffusionImageId: null,
  diffusionProfileIndex: null,
  diffusionSettings: null,
  diffusionSource: null,
  diffusionPollTimer: null,
  diffusionPreviewTimer: null,
  diffusionPreviewRequestId: 0,
  diffusionRequestedSignature: "",
  diffusionController: null,
  originalShare: {
    imageId: null,
    file: null,
    promise: null,
    busyImageId: null,
    retryImageId: null,
    openImageId: null,
  },
};

const RETOUCH_SAVE_DEBOUNCE_MS = 1200;
const RETOUCH_TEMPERATURE_DELTA_LIMIT = 2500;
const RETOUCH_OFFSET_DELTA_LIMIT = 100;
const HISTOGRAM_SAMPLE_LONG_EDGE = 512;
const HISTOGRAM_RETOUCH_DEBOUNCE_MS = 100;
const COMPRESSED_REVIEW_PREVIEW_LONG_EDGE = 2048;
const TOUCH_SWIPE_MIN_PX = 72;
const TOUCH_SWIPE_RATIO = 1.65;
const ZOOM_LONG_PRESS_MS = 380;
const ZOOM_MOVE_CANCEL_PX = 22;
const ZOOM_LOUPE_TOUCH_GAP_PX = 28;
const ZOOM_LOUPE_POINTER_GAP_PX = 18;
const WHEEL_NAV_THRESHOLD_PX = 90;
const WHEEL_NAV_RESET_MS = 220;
const WHEEL_NAV_COOLDOWN_MS = 260;
const RATING_VALUES = [0, 1, 2, 3, 4, 5];
const COLOR_LABELS = ["red", "yellow", "green", "blue", "purple"];
const BW_FILTERS = ["none", "yellow", "orange", "red", "green"];
const BW_FILTER_LABELS = new Map([
  ["none", "None"],
  ["yellow", "Y"],
  ["orange", "O"],
  ["red", "R"],
  ["green", "G"],
]);
const BW_FILTER_NAMES = new Map([
  ["none", "No"],
  ["yellow", "Yellow"],
  ["orange", "Orange"],
  ["red", "Red"],
  ["green", "Green"],
]);
const CROP_RATIO_PRESETS = [
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
const PANORAMA_MATCHING_MODES = [
  ["automatic", "Automatic"],
  ["sequential", "Sequential"],
  ["multi-row", "Multi-row"],
  ["flat-mosaic", "Flat mosaic"],
];
const PANORAMA_PROJECTIONS = [
  ["rectilinear", "Rectilinear"],
  ["cylindrical", "Cylindrical"],
  ["equirectangular", "Equirectangular"],
  ["panini", "General Panini"],
];
const SAMPLER_POLL_MS = 500;
const SAMPLER_PRIORITY_DEBOUNCE_MS = 60;
const DIFFUSION_POLL_MS = 500;
const DIFFUSION_PREVIEW_DEBOUNCE_MS = 280;
const DIFFUSION_METHODS = [
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
const DIFFUSION_PRESETS = [
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
const DIFFUSION_DETAIL_AREAS = [
  { kind: "focus", label: "Focus area" },
  { kind: "high-contrast-highlight", label: "High-contrast highlight" },
  { kind: "broad-highlight", label: "Broad highlight" },
];

let wheelNavigation = {
  axis: null,
  amount: 0,
  lastAt: 0,
  lockedUntil: 0,
};

preactRender(h(ReviewShell), document.getElementById("review-root"));

function ReviewShell() {
  return h(
    Fragment,
    null,
    h(
      "div",
      { class: "app" },
      h(
        "aside",
        { class: "sidebar" },
        h(
          "header",
          { class: "sidebar-header" },
          h("div", null, h("h1", null, "Review"), h("div", { id: "app-version", class: "app-version" }, "mini-film")),
          h(
            "div",
            { class: "header-actions" },
            h(
              "button",
              { id: "shortcuts-help", class: "help-button", type: "button", "aria-label": "Keyboard shortcuts" },
              "?",
            ),
            h(
              "button",
              { id: "publish", class: "publish-button", type: "button", title: "Publish", "aria-label": "Publish" },
              "Pub",
            ),
          ),
        ),
        h(
          "div",
          { class: "filter" },
          h(
            "label",
            null,
            h("span", null, "Show rating >="),
            h(
              "select",
              { id: "min-rating" },
              RATING_VALUES.map((rating) => h("option", { key: rating, value: String(rating) }, String(rating))),
            ),
          ),
        ),
        h(
          "div",
          { class: "status" },
          h("span", { id: "live-dot", class: "live-dot" }),
          h("span", { id: "status" }, "Connecting..."),
        ),
        h("div", { id: "image-list", class: "image-list" }),
        h(
          "section",
          { class: "sidebar-tools", "aria-label": "Tools" },
          h("div", { class: "sidebar-tools-title" }, "Tools"),
          h(
            "button",
            {
              id: "crop-toggle",
              class: "sidebar-tool-button",
              type: "button",
              disabled: true,
            },
            "Crop/rotate",
          ),
          h(
            "button",
            {
              id: "diffusion",
              class: "sidebar-tool-button",
              type: "button",
              title: "Open diffusion tools",
              "aria-label": "Open diffusion tools",
              disabled: true,
            },
            "Diffusion",
          ),
          h(
            "button",
            {
              id: "sampler",
              class: "sidebar-tool-button",
              type: "button",
              title: "Open profile sampler",
              "aria-label": "Open profile sampler",
              hidden: true,
            },
            "Sampler",
          ),
          h(
            "button",
            {
              id: "panorama",
              class: "sidebar-tool-button",
              type: "button",
              title: "Create panorama",
              "aria-label": "Create panorama",
              hidden: true,
            },
            "Panorama",
          ),
        ),
      ),
      h(
        "main",
        { class: "workspace" },
        h(
          "section",
          { class: "viewer" },
          h("div", { id: "empty", class: "empty" }, "Waiting for pictures"),
          h("img", { id: "main-image", alt: "", draggable: false, decoding: "async", fetchpriority: "high" }),
          h("svg", {
            id: "focus-overlay",
            class: "focus-overlay",
            hidden: true,
            viewBox: "0 0 1000 1000",
            preserveAspectRatio: "none",
            "aria-label": "Camera focus points",
          }),
          h(
            "div",
            { id: "crop-stage", class: "crop-stage", hidden: true },
            h(
              "div",
              { id: "crop-canvas", class: "crop-canvas" },
              h("img", {
                id: "crop-source-image",
                class: "crop-source-image",
                alt: "",
                draggable: false,
                decoding: "async",
              }),
              h("img", {
                id: "crop-current-image",
                class: "crop-current-image",
                alt: "",
                draggable: false,
                decoding: "async",
              }),
              h(
                "div",
                { id: "crop-overlay", class: "crop-overlay", hidden: true },
                h(
                  "div",
                  { id: "crop-box", class: "crop-box" },
                  h("span", { "data-crop-handle": "nw" }),
                  h("span", { "data-crop-handle": "ne" }),
                  h("span", { "data-crop-handle": "sw" }),
                  h("span", { "data-crop-handle": "se" }),
                ),
              ),
            ),
          ),
          h("div", { id: "gesture-feedback", class: "gesture-feedback", hidden: true }),
          h("div", { id: "zoom-full", class: "zoom-full", hidden: true, "aria-hidden": "true" }),
          h("div", { id: "zoom-loupe", class: "zoom-loupe", hidden: true }),
          h(
            "div",
            { id: "histogram-overlay", class: "histogram-overlay", hidden: true, "aria-label": "Histogram" },
            h("canvas", { id: "histogram-canvas", width: "512", height: "128" }),
            h("div", { id: "histogram-empty", class: "histogram-empty", hidden: true }, "No image"),
          ),
          h("div", { id: "retouch-grid", class: "retouch-grid", hidden: true }),
          h(
            "div",
            { id: "crop-tools", class: "crop-tools", hidden: true },
            h("button", { id: "crop-rotate-left", type: "button" }, "-90"),
            h(
              "label",
              { class: "crop-rotation-control" },
              h("span", null, "Rotate"),
              h("input", { id: "crop-rotation", type: "range", min: "-180", max: "180", step: "0.25", value: "0" }),
              h("output", { id: "crop-rotation-value" }, "0"),
            ),
            h("button", { id: "crop-rotate-right", type: "button" }, "+90"),
            h(
              "label",
              { class: "crop-ratio-control" },
              h("span", null, "Ratio"),
              h(
                "select",
                { id: "crop-ratio" },
                h("option", { value: "current", hidden: true }, "Current"),
                CROP_RATIO_PRESETS.map(([value, label]) => h("option", { key: value, value }, label)),
              ),
            ),
            h(
              "div",
              { id: "crop-actions", class: "crop-actions", hidden: true, "aria-label": "Crop actions" },
              h("button", { id: "crop-reset", type: "button" }, "Clear"),
              h("button", { id: "crop-cancel", type: "button" }, "Cancel"),
              h("button", { id: "crop-ok", class: "crop-apply", type: "button" }, "OK"),
            ),
          ),
        ),
        h(
          "section",
          { class: "panel" },
          h(
            "div",
            { class: "meta" },
            h(
              "div",
              null,
              h(
                "div",
                { class: "image-title-line" },
                h("div", { id: "image-title", class: "image-title" }),
                h("div", { id: "image-exif", class: "image-exif", "aria-label": "Camera settings" }),
              ),
            ),
            h("div", { id: "profile-state", class: "profile-state" }),
          ),
          h(
            "div",
            { class: "mobile-actions", "aria-label": "Review tools" },
            h("button", { "data-mobile-drawer": "profiles", type: "button" }, "Profiles"),
            h("button", { "data-mobile-drawer": "retouch", type: "button" }, "Retouch"),
            h("button", { id: "mobile-save-original", type: "button", hidden: true }, "Save Photo"),
            h("button", { "data-mobile-drawer": "metadata", type: "button" }, "Meta"),
            h("button", { id: "mobile-publish", type: "button" }, "Publish"),
          ),
          h("div", { id: "profiles", class: "profiles" }),
          h(ControlsShell),
        ),
      ),
      h("div", { id: "sampler-overlay", class: "sampler-overlay", hidden: true }),
      h("div", { id: "diffusion-overlay", class: "diffusion-overlay", hidden: true }),
    ),
    h(ShortcutsOverlay),
    h("div", { id: "command-invocation-overlay", class: "command-invocation-overlay", hidden: true }),
    h("div", { id: "profile-info-overlay", class: "profile-info-overlay", hidden: true }),
    h(PublishOverlay),
    h("div", { id: "panorama-overlay", class: "panorama-overlay", hidden: true }),
  );
}

function ProfileInfoOverlay() {
  const profile = profileByIndex(state.profileInfoProfileIndex);
  if (!profile) return null;
  const metadata = profile.metadata || {};
  const image = findImage(state.currentId);
  const exif = image?.exif || {};
  const haldImage = metadata.has_hald ? `api/profile/${profile.index}/hald` : null;
  const pp3Url = image ? profilePp3Url(image, profile) : null;
  const pp3Key = image ? profilePp3Key(image, profile) : null;
  const pp3State = state.profileInfoPp3.key === pp3Key ? state.profileInfoPp3 : null;
  const pp3Text = pp3State?.error || pp3State?.text || "Loading...";
  return h(
    "section",
    { class: "profile-info-card", role: "dialog", "aria-modal": "true" },
    h(
      "header",
      { class: "profile-info-header" },
      h("div", null, h("h3", null, "Profile info"), h("p", null, profileDisplayName(profile))),
      h(
        "button",
        {
          class: "profile-info-close",
          type: "button",
          "aria-label": "Close profile info",
          onClick: (event) => {
            event.preventDefault();
            closeProfileInfo();
          },
        },
        "×",
      ),
    ),
    h(
      "div",
      { class: "profile-info-grid" },
      ProfileInfoRow("Profile", metadata.profile_name || "—"),
      ProfileInfoRow("Profile UUID", metadata.profile_uuid || "—"),
      ProfileInfoRow("Look", metadata.look_name || "—"),
      ProfileInfoRow("Look UUID", metadata.look_uuid || "—"),
      ProfileInfoRow("Source profile", metadata.source_profile_name || "—"),
      ProfileInfoRow("Source UUID", metadata.source_profile_uuid || "—"),
      ProfileInfoRow("Active D-Lighting", exif.active_d_lighting || "—"),
      ProfileInfoRow("Source adjustments", renderAdjustments(metadata.source_adjustments || {}), true),
      ProfileInfoRow("Emulation adjustments", renderAdjustments(metadata.emulation_adjustments || {}), true),
      ProfileInfoRow("Source sharpening", renderSharpening(metadata.source_sharpening || {}), true),
      ProfileInfoRow("Emulation sharpening", renderSharpening(metadata.emulation_sharpening || {}), true),
      ProfileInfoRow("PP3 adjustments", renderPp3Adjustments(metadata.pp3_adjustments || []), true),
      ProfileInfoRow("Has Camera Raw settings", metadata.has_camera_raw_settings ? "Yes" : "No"),
      ProfileInfoRow("Has HALD LUT", metadata.has_hald ? "Yes" : "No"),
      ProfileInfoRow("Has PP3", metadata.has_pp3 ? "Yes" : "No"),
      ProfileInfoRow("Grain", renderGrain(metadata.grain)),
      ProfileInfoRow("PP3 file", metadata.pp3_name || "—"),
    ),
    haldImage
      ? h(
          "div",
          { class: "profile-info-hald" },
          h("img", {
            src: versionedUrl(haldImage, state.data?.version || ""),
            alt: "HALD LUT table",
            loading: "lazy",
          }),
        )
      : null,
    pp3Url
      ? h(
          "details",
          {
            key: pp3Key,
            class: "profile-info-details",
            onToggle: (event) => {
              if (event.currentTarget.open) loadProfilePp3(image, profile);
            },
          },
          h("summary", null, "Complete PP3"),
          h(
            "div",
            { class: "profile-info-pp3-actions" },
            h(
              "a",
              {
                href: reviewUrl(pp3Url),
                download: profilePp3DownloadName(image, profile),
                class: "profile-info-pp3-download",
              },
              "Download PP3",
            ),
          ),
          h("pre", { class: `profile-info-pp3 ${pp3State?.error ? "profile-info-pp3-error" : ""}` }, pp3Text),
        )
      : null,
    h(
      "details",
      { class: "profile-info-details" },
      h("summary", null, "Advanced metadata"),
      h("pre", { class: "profile-info-json" }, JSON.stringify(metadata, null, 2)),
    ),
  );
}

function ProfileInfoRow(label, value, multiline = false) {
  if (value === null || value === undefined || value === "") {
    value = "—";
  }
  return h(
    "div",
    { class: `profile-info-row ${multiline ? "profile-info-row-multiline" : ""}` },
    h("span", { class: "profile-info-label" }, label),
    h(
      "span",
      { class: "profile-info-value" },
      typeof value === "string" ? value : h("code", { class: "profile-info-pre" }, value),
    ),
  );
}

function renderAdjustments(adjustments) {
  const values = [
    ["exposure", adjustments.exposure],
    ["contrast", adjustments.contrast],
    ["highlights", adjustments.highlights],
    ["shadows", adjustments.shadows],
    ["whites", adjustments.whites],
    ["blacks", adjustments.blacks],
    ["saturation", adjustments.saturation],
    ["vibrance", adjustments.vibrance],
    ["clarity", adjustments.clarity],
  ].map(([key, value]) => `${key}: ${formatNumberField(value, 2)}`);
  return values.join("\n");
}

function renderGrain(grain) {
  if (!grain || !grain.amount) {
    return "off";
  }
  const size = Number(grain.size);
  const frequency = Number(grain.frequency);
  if (!Number.isFinite(size) || !Number.isFinite(frequency)) {
    return "off";
  }
  return `amount=${grain.amount}, size=${size}, frequency=${frequency}`;
}

function renderSharpening(sharpening) {
  const values = [
    ["present", sharpening.present],
    ["amount", sharpening.amount],
    ["radius", sharpening.radius],
    ["detail", sharpening.detail],
    ["masking", sharpening.masking],
  ].map(([key, value]) => `${key}: ${formatNumberField(value, 2)}`);
  return values.join("\n");
}

function renderPp3Adjustments(sections) {
  if (!Array.isArray(sections) || sections.length === 0) {
    return "—";
  }
  return sections
    .map((section) => {
      const source = section.source ? `${section.source} ` : "";
      const entries = Array.isArray(section.entries)
        ? section.entries
            .filter((entry) => entry?.key && entry?.value)
            .map((entry) => `${entry.key}=${entry.value}`)
            .join(", ")
        : "";
      return entries ? `${source}[${section.section}]\n${entries}` : "";
    })
    .filter(Boolean)
    .join("\n\n");
}

function formatNumberField(value, digits = 2) {
  const number = Number(value);
  if (Number.isFinite(number)) {
    return number.toLocaleString("en-US", {
      maximumFractionDigits: digits,
      useGrouping: false,
    });
  }
  return String(value ?? "—");
}

function profileByIndex(profileIndex) {
  if (profileIndex === null || profileIndex === undefined) return null;
  return (state.data?.profiles || []).find((profile) => profile.index === profileIndex) || null;
}

function openProfileInfo(profile) {
  closeCommandInvocation();
  clearProfileInfoPp3();
  state.profileInfoProfileIndex = profileRenderIndex(profile);
  renderProfileInfo();
}

function closeProfileInfo() {
  if (state.profileInfoProfileIndex === null) return;
  state.profileInfoProfileIndex = null;
  clearProfileInfoPp3();
  renderProfileInfo();
}

function profilePp3Url(image, profile) {
  return `api/profile/${profile.index}/pp3/${image.id}`;
}

function profilePp3Key(image, profile) {
  return `${image.id}:${profile.index}:${image.updated_at || ""}`;
}

function profilePp3DownloadName(image, profile) {
  const rawName = image.file_name || image.relative_path || "mini-film";
  const baseName = rawName.replace(/\.[^.]*$/, "");
  const profileName = profile.stem || profile.selector || profileDisplayName(profile);
  return `${safeDownloadPart(baseName)}--${safeDownloadPart(profileName)}.pp3`;
}

function clearProfileInfoPp3() {
  state.profileInfoPp3 = {
    key: null,
    text: null,
    error: null,
    loading: false,
    requestId: state.profileInfoPp3.requestId + 1,
  };
}

async function loadProfilePp3(image, profile) {
  const key = profilePp3Key(image, profile);
  if (
    state.profileInfoPp3.key === key &&
    (state.profileInfoPp3.loading || state.profileInfoPp3.text !== null || state.profileInfoPp3.error !== null)
  ) {
    return;
  }

  const requestId = state.profileInfoPp3.requestId + 1;
  state.profileInfoPp3 = {
    key,
    text: null,
    error: null,
    loading: true,
    requestId,
  };
  renderProfileInfo();
  try {
    const response = await fetch(reviewUrl(profilePp3Url(image, profile)), { cache: "no-store" });
    const body = await response.text();
    if (!response.ok) {
      let message = `PP3 ${response.status}`;
      try {
        message = JSON.parse(body).error || message;
      } catch {
        if (body.trim()) message = body.trim();
      }
      throw new Error(message);
    }
    if (state.profileInfoPp3.requestId !== requestId || state.profileInfoPp3.key !== key) return;
    state.profileInfoPp3 = { key, text: body, error: null, loading: false, requestId };
  } catch (error) {
    if (state.profileInfoPp3.requestId !== requestId || state.profileInfoPp3.key !== key) return;
    state.profileInfoPp3 = {
      key,
      text: null,
      error: `Could not load PP3: ${error.message}`,
      loading: false,
      requestId,
    };
  }
  renderProfileInfo();
}

function renderProfileInfo() {
  if (!els.profileInfoOverlay) return;
  const overlayContent = ProfileInfoOverlay();
  if (!overlayContent) {
    preactRender(null, els.profileInfoOverlay);
    els.profileInfoOverlay.setAttribute("hidden", "hidden");
    return;
  }
  preactRender(overlayContent, els.profileInfoOverlay);
  els.profileInfoOverlay.removeAttribute("hidden");
}

function CommandInvocationOverlay() {
  const invocation = state.data?.invocation || "Invocation unavailable.";
  const lines = commandInvocationLines(invocation);
  return h(
    "section",
    { class: "command-invocation-card", role: "dialog", "aria-modal": "true" },
    h(
      "header",
      { class: "command-invocation-header" },
      h("div", null, h("h3", null, "Command invocation"), h("p", null, "This review session was launched with:")),
      h(
        "button",
        {
          class: "command-invocation-close",
          type: "button",
          "aria-label": "Close command invocation",
          onClick: (event) => {
            event.preventDefault();
            closeCommandInvocation();
          },
        },
        "×",
      ),
    ),
    h(
      "div",
      {
        class: "command-invocation-code",
        title: invocation,
      },
      lines.length === 0
        ? invocation
        : lines.map((line, index, arr) =>
            h(
              "div",
              {
                class: `command-invocation-line${index === 0 ? "" : " command-invocation-line-indented"}`,
                "aria-label":
                  line.type === "single"
                    ? line.value
                    : line.type === "binary-subcommand"
                      ? `${line.value} ${line.subcommand}`
                      : `${line.name} ${line.value}`,
              },
              h(
                "span",
                { class: "command-invocation-line-content" },
                line.type === "binary-subcommand"
                  ? [
                      h("span", { class: "command-invocation-binary" }, line.value),
                      h("span", { class: "command-invocation-arg" }, line.subcommand),
                    ]
                  : line.type === "pair"
                    ? [
                        h("span", { class: "command-invocation-arg" }, line.name),
                        h(
                          "span",
                          { class: "command-invocation-value" },
                          commandInvocationDisplayValue(
                            line.value,
                            line.name === "--profile" || line.name === "-p" || line.name === "--profile-name",
                          ),
                        ),
                      ]
                    : [
                        h(
                          "span",
                          {
                            class: line.binary ? "command-invocation-binary" : "command-invocation-arg",
                          },
                          line.value,
                        ),
                      ],
              ),
              index < arr.length - 1 ? h("span", { class: "command-invocation-continuation" }, " \\") : null,
            ),
          ),
    ),
    h(
      "div",
      { class: "command-invocation-actions" },
      h(
        "button",
        {
          type: "button",
          onClick: () => {
            const copyText = commandInvocationCopyText(state.data?.invocation ?? "");
            if (!copyText) return;
            void navigator.clipboard?.writeText(copyText).catch((error) => {
              console.error(error);
            });
          },
        },
        "Copy",
      ),
      h(
        "button",
        {
          type: "button",
          onClick: () => closeCommandInvocation(),
        },
        "Close",
      ),
    ),
  );
}

function commandInvocationLines(invocation) {
  const tokens = commandInvocationTokens(invocation);
  const lines = [];
  for (let index = 0; index < tokens.length;) {
    const token = tokens[index];
    if (index === 0) {
      if (token !== "" && (tokens[index + 1] === "app" || tokens[index + 1] === "daemon")) {
        lines.push({
          type: "binary-subcommand",
          value: token,
          subcommand: tokens[index + 1],
        });
        index += 2;
        continue;
      }
      lines.push({
        type: "single",
        value: token,
        binary: true,
        name: "binary",
      });
      index += 1;
      continue;
    }

    if (token.startsWith("--") && tokens[index + 1] && !tokens[index + 1].startsWith("--")) {
      let nextIndex = index + 1;
      while (nextIndex < tokens.length && !tokens[nextIndex].startsWith("--")) {
        nextIndex += 1;
      }

      const rawValue = tokens.slice(index + 1, nextIndex);
      lines.push({
        type: "pair",
        name: token,
        value: rawValue.join(" "),
      });
      index = nextIndex;
      continue;
    }

    lines.push({
      type: "single",
      value: token,
    });
    index += 1;
  }
  return lines;
}

function commandInvocationTokens(invocation) {
  const tokens = [];
  let current = "";
  let quote = null;
  let escaped = false;

  for (let index = 0; index < invocation.length; index++) {
    const char = invocation[index];
    const next = invocation[index + 1];

    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }

    if (char === "\\") {
      if (quote === "'") {
        current += char;
      } else if (next !== undefined) {
        current += next;
        index += 1;
      } else {
        current += "\\";
      }
      continue;
    }

    if (quote) {
      if (char === quote) {
        quote = null;
        continue;
      }
      if (quote === '"' && char === "\\") {
        if (next === "\\" || next === '"' || next === "$" || next === "`") {
          current += next;
          index += 1;
          continue;
        }
      }
      current += char;
      continue;
    }

    if (char === "'" || char === '"') {
      quote = char;
      continue;
    }
    if (/\s/.test(char)) {
      if (current.length > 0) {
        tokens.push(current);
        current = "";
      }
      continue;
    }

    current += char;
  }

  if (current.length > 0) tokens.push(current);
  return tokens;
}

function commandInvocationDisplayValue(value, forceQuote = false) {
  const str = String(value);
  if (!forceQuote && (/^[-+]?\d+(?:\.\d+)?$/.test(str) || !/[\\s"]/.test(str))) {
    return str;
  }
  const escaped = str.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `"${escaped}"`;
}

function commandInvocationCopyText(invocation) {
  const tokens = commandInvocationTokens(invocation);
  return tokens.map(commandInvocationShellEscape).join(" ");
}

function commandInvocationShellEscape(value) {
  const raw = String(value);
  if (raw.length === 0) return "''";
  if (/^[A-Za-z0-9._/+=:@,-]+$/.test(raw)) return raw;
  if (!raw.includes("'")) {
    return `'${raw}'`;
  }
  return `'${raw.replace(/'/g, "'\"'\"'")}'`;
}

function openCommandInvocation() {
  closeProfileInfo();
  state.commandInvocationOpen = true;
  renderCommandInvocation();
}

function closeCommandInvocation() {
  if (!state.commandInvocationOpen) return;
  state.commandInvocationOpen = false;
  renderCommandInvocation();
}

function renderCommandInvocation() {
  if (!els.commandInvocationOverlay) return;
  if (!state.commandInvocationOpen) {
    preactRender(null, els.commandInvocationOverlay);
    els.commandInvocationOverlay.setAttribute("hidden", "hidden");
    return;
  }
  preactRender(h(CommandInvocationOverlay), els.commandInvocationOverlay);
  els.commandInvocationOverlay.removeAttribute("hidden");
}

function profileRenderIndex(profile) {
  if (profile?.index !== undefined && Number.isFinite(Number(profile.index))) return Number(profile.index);
  if (profile?.profile_index !== undefined && Number.isFinite(Number(profile.profile_index)))
    return Number(profile.profile_index);
  return null;
}

function ControlsShell() {
  return h(
    "div",
    { class: "controls" },
    h(
      "div",
      { class: "rating", role: "group", "aria-label": "Rating" },
      RATING_VALUES.map((rating) =>
        h("button", { key: rating, "data-rating": String(rating), type: "button" }, String(rating)),
      ),
    ),
    h(
      "div",
      { class: "labels", role: "group", "aria-label": "Label" },
      COLOR_LABELS.map((label) =>
        h(
          "button",
          {
            key: label,
            "data-label": label,
            type: "button",
            title: capitalize(label),
            "aria-label": `${capitalize(label)} label`,
          },
          labelLetter(label),
        ),
      ),
    ),
    h(
      "label",
      { class: "tags" },
      h("span", null, "Tags"),
      h("input", { id: "tags", type: "text", inputMode: "numeric", autocomplete: "off", placeholder: "12, 42, 108" }),
    ),
    h(
      "label",
      { class: "notes" },
      h("span", null, "Notes"),
      h("input", { id: "notes", type: "text", autocomplete: "off", placeholder: "optional note" }),
    ),
    h(
      "section",
      { class: "retouch", "aria-label": "Retouch" },
      h(
        "div",
        { class: "retouch-header" },
        h("span", null, "Retouch"),
        h(
          "div",
          { class: "retouch-header-actions" },
          h("button", { id: "retouch-copy", type: "button" }, "Copy"),
          h("button", { id: "retouch-paste", type: "button" }, "Paste"),
          h("button", { id: "retouch-reset", type: "button" }, "Reset"),
        ),
      ),
      h(RetouchSlider, { id: "retouch-clarity", label: "Clarity", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, {
        id: "retouch-highlights",
        label: "Highlights",
        min: "-100",
        max: "100",
        step: "1",
        value: "0",
      }),
      h(RetouchSlider, { id: "retouch-whites", label: "Whites", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, {
        id: "retouch-temperature",
        label: "Temperature",
        min: "-2500",
        max: "2500",
        step: "50",
        value: "0",
        output: "0K",
      }),
      h(RetouchSlider, { id: "retouch-exposure", label: "Exposure", min: "-4", max: "4", step: "0.05", value: "0" }),
      h(RetouchSlider, { id: "retouch-contrast", label: "Contrast", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, { id: "retouch-shadows", label: "Shadows", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, { id: "retouch-blacks", label: "Blacks", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, { id: "retouch-offset", label: "Tint", min: "-100", max: "100", step: "1", value: "0" }),
    ),
  );
}

function RetouchSlider({ id, label, min, max, step, value, output = value }) {
  return h(
    "label",
    { "data-retouch-adjustment": "true" },
    h("span", null, label),
    h("input", { id, type: "range", min, max, step, value }),
    h("output", { id: `${id}-value` }, output),
  );
}

function ShortcutsOverlay() {
  const sections = [
    [
      "Pictures",
      [
        [["←", "→"], "Previous or next picture without changing the rating."],
        [["Enter"], "Move to the next picture without changing the rating."],
      ],
    ],
    ["Histogram", [[["h"], "Show or hide the luma and RGB histogram."]]],
    ["Information", [[["i"], "Show or hide camera focus points on the picture."]]],
    [
      "Touch / Mouse",
      [
        [["Swipe ←/→"], "Move between visible pictures without changing the rating."],
        [["Swipe ↑/↓"], "Change the rating without advancing."],
        [["Wheel ←/→"], "Move between visible pictures after a short scroll threshold."],
        [["Wheel ↑/↓"], "Preview the previous or next profile after a short scroll threshold."],
        [["Hold"], "Show a nearby loupe for the picture under the cursor or finger until released."],
        [["Double-click"], "Toggle full-image zoom; move the cursor to pan the zoomed image."],
        [
          ["Profile"],
          "Click a profile thumbnail to preview it; use its checkbox to make it available for this picture. Double-click or double-tap a profile to make only that profile available.",
        ],
      ],
    ],
    [
      "Rating",
      [
        [["`", "§", "1", "2", "3", "4", "5"], "Set rating and advance to the next visible picture."],
        [["↑", "↓"], "Increase or decrease the rating, then advance."],
      ],
    ],
    [
      "Labels",
      [
        [["6", "7", "8", "9", "0"], "Toggle red, yellow, green, blue, or purple labels without advancing."],
        [["r", "y", "g", "b", "p"], "Same label toggles using mnemonic keys."],
        [["n"], "Clear all color labels."],
      ],
    ],
    [
      "Adjustments",
      [
        [["c"], "Copy the current retouch slider adjustments."],
        [["v"], "Paste copied retouch slider adjustments to the current picture."],
      ],
    ],
    [
      "Profiles",
      [
        [["PgUp", "PgDn"], "Preview the previous or next profile for the current picture."],
        [["Space"], "Enable or disable the selected profile for the current picture."],
        [["Double-click"], "Enable only that profile thumbnail for the current picture."],
      ],
    ],
    [
      "Metadata",
      [
        [[","], "Focus tags."],
        [["/"], "Focus notes."],
        [["Enter"], "Save tags and advance; save notes and return to review."],
        [["Esc"], "Save tags or notes and return to review without advancing."],
      ],
    ],
    [
      "View",
      [
        [["f"], "Toggle fullscreen."],
        [["?", "Esc"], "Show or hide this shortcuts overlay."],
      ],
    ],
    ["Retouch", [[["Double-click"], "Double-click a retouch control name to reset that value."]]],
    [
      "Tools",
      [
        [["Crop", "OK"], "Open crop/rotate from Tools, adjust the frame, then apply it with OK."],
        [["Diffusion"], "Preview film-like softness and highlight glow for the selected profile."],
        [["r"], "Rotate the selected crop ratio while crop mode is open."],
      ],
    ],
  ];
  return h(
    "div",
    {
      id: "shortcuts-overlay",
      class: "shortcuts-overlay",
      role: "dialog",
      "aria-modal": "true",
      "aria-labelledby": "shortcuts-title",
      hidden: true,
    },
    h(
      "section",
      { class: "shortcuts-card" },
      h(
        "header",
        { class: "shortcuts-header" },
        h("h2", { id: "shortcuts-title" }, "Shortcuts"),
        h("button", { id: "shortcuts-close", type: "button" }, "Close"),
      ),
      h(
        "div",
        { class: "shortcut-sections" },
        sections.map(([title, rows]) =>
          h(
            "section",
            { key: title, class: "shortcut-section" },
            h("h3", null, title),
            rows.map(([keys, description]) =>
              h(
                "div",
                { key: `${title}-${description}`, class: "shortcut-row" },
                h(
                  "span",
                  { class: "shortcut-keys" },
                  keys.map((key) => h("kbd", { key }, key)),
                ),
                h("span", null, description),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

function PublishOverlay() {
  return h(
    "div",
    {
      id: "publish-overlay",
      class: "publish-overlay",
      role: "dialog",
      "aria-modal": "true",
      "aria-labelledby": "publish-title",
      hidden: true,
    },
    h(
      "form",
      { id: "publish-form", class: "publish-card" },
      h(
        "header",
        { class: "publish-header" },
        h(
          "div",
          null,
          h("h2", { id: "publish-title" }, "Publish"),
          h("p", { id: "publish-mode" }, "Link existing reviewed outputs when settings match daemon defaults."),
          h("p", { id: "publish-count", class: "publish-count" }),
        ),
        h("button", { id: "publish-cancel", type: "button" }, "Cancel"),
      ),
      h(
        "div",
        { class: "publish-grid" },
        h(PublishSelectionSection),
        h(PublishOutputSection),
        h(PublishGallerySection),
      ),
      h(
        "footer",
        { class: "publish-footer" },
        h("div", { id: "publish-status", class: "publish-status" }),
        h("button", { id: "publish-submit", type: "submit" }, "Start publish job"),
      ),
    ),
  );
}

function PublishSelectionSection() {
  return h(
    "section",
    { class: "publish-section" },
    h("h3", null, "Selection"),
    h(
      "label",
      null,
      h("span", null, "Output folder"),
      h("input", { id: "publish-album", type: "text", autocomplete: "off" }),
    ),
    h(
      "label",
      null,
      h("span", null, "Rating >="),
      h(
        "select",
        { id: "publish-min-rating" },
        RATING_VALUES.map((rating) => h("option", { key: rating, value: String(rating) }, String(rating))),
      ),
    ),
    h(
      "div",
      { class: "publish-labels" },
      h("span", null, "Colour labels"),
      COLOR_LABELS.map((label) =>
        h(
          "label",
          { key: label, "data-publish-label": label, title: capitalize(label) },
          h("input", { type: "checkbox", name: "publish-label", value: label }),
          " ",
          h("span", null, capitalize(label)),
        ),
      ),
    ),
    h(
      "label",
      null,
      h("span", null, "Tags"),
      h("input", {
        id: "publish-tags",
        type: "text",
        autocomplete: "off",
        placeholder: "optional comma-separated tags",
      }),
    ),
  );
}

function PublishOutputSection() {
  return h(
    "section",
    { class: "publish-section" },
    h("h3", null, "Output"),
    h(
      "label",
      null,
      h("span", null, "Format"),
      h(
        "select",
        { id: "publish-output-format" },
        h("option", { value: "jpg" }, "JPG"),
        h("option", { value: "tiff" }, "TIFF"),
      ),
    ),
    h(
      "label",
      null,
      h("span", null, "Grain engine"),
      h(
        "select",
        { id: "publish-grain-engine" },
        h("option", { value: "legacy" }, "Legacy"),
        h("option", { value: "rfgrfast" }, "RFGR fast"),
        h("option", { value: "rfgr" }, "RFGR"),
      ),
    ),
    h(
      "label",
      null,
      h("span", null, "Grain reference MPix"),
      h("input", {
        id: "publish-normalize-grain-mpix",
        type: "number",
        min: "5e-324",
        step: "any",
        required: true,
      }),
    ),
    h(
      "label",
      null,
      h("span", null, "Size"),
      h(
        "select",
        { id: "publish-size-mode" },
        h("option", { value: "original" }, "Original size"),
        h("option", { value: "long-edge" }, "Long edge"),
        h("option", { value: "bounds" }, "Max width/height"),
        h("option", { value: "geometry" }, "Convert geometry"),
      ),
    ),
    h(
      "div",
      { class: "publish-sizes" },
      h("input", { id: "publish-long-edge", type: "number", min: "1", placeholder: "long edge px" }),
      h("input", { id: "publish-max-width", type: "number", min: "1", placeholder: "max width" }),
      h("input", { id: "publish-max-height", type: "number", min: "1", placeholder: "max height" }),
      h("input", { id: "publish-resize", type: "text", placeholder: "e.g. 3840x3840>" }),
    ),
    h(
      "label",
      null,
      h("span", null, "JPG quality"),
      h("input", { id: "publish-jpg-quality", type: "number", min: "1", max: "100" }),
    ),
    h(
      "label",
      null,
      h("span", null, "Subsampling"),
      h(
        "select",
        { id: "publish-jpeg-subsampling" },
        h("option", { value: "s444" }, "4:4:4"),
        h("option", { value: "s422" }, "4:2:2"),
        h("option", { value: "s420" }, "4:2:0"),
      ),
    ),
    h(
      "div",
      { class: "publish-checks" },
      h("label", null, h("input", { id: "publish-normalize-grain", type: "checkbox" }), " Normalize grain"),
      h("label", null, h("input", { id: "publish-progressive", type: "checkbox" }), " Progressive JPG"),
      h("label", null, h("input", { id: "publish-strip-metadata", type: "checkbox" }), " Strip metadata"),
    ),
  );
}

function PublishGallerySection() {
  return h(
    "section",
    { class: "publish-section publish-section-wide" },
    h("h3", null, "Gallery"),
    h(
      "label",
      null,
      h("span", null, "Template"),
      h(
        "select",
        { id: "publish-gallery" },
        [
          ["none", "No gallery"],
          ["modern", "Modern"],
          ["soft", "Soft"],
          ["compact", "Compact"],
          ["hero", "Hero"],
          ["phone", "Phone"],
          ["all", "All templates"],
        ].map(([value, label]) => h("option", { key: value, value }, label)),
      ),
    ),
    h(
      "label",
      null,
      h("span", null, "Columns"),
      h("input", { id: "publish-gallery-columns", type: "number", min: "1", max: "12" }),
    ),
    h(
      "label",
      null,
      h("span", null, "Thumbnail edge"),
      h("input", { id: "publish-gallery-thumbnail-long-edge", type: "number", min: "1" }),
    ),
  );
}

function PanoramaOverlay() {
  const project = currentPanoramaProject();
  const projects = state.data?.panorama?.projects || [];
  const images = state.data?.images || [];
  const busy = Boolean(state.data?.panorama?.busy);
  const operationRunning = ["previewing", "rendering"].includes(project?.status);
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

  return h(
    "section",
    { class: "panorama-card", role: "dialog", "aria-modal": "true", "aria-labelledby": "panorama-title" },
    h(
      "header",
      { class: "panorama-header" },
      h(
        "div",
        null,
        h("h2", { id: "panorama-title" }, "Panorama"),
        h(
          "p",
          null,
          `${state.panoramaImageIds.length} selected | ${project ? capitalize(project.status) : "New project"}`,
        ),
      ),
      h(
        "div",
        { class: "panorama-header-actions" },
        h(
          "select",
          {
            value: state.panoramaProjectId === null ? "new" : String(state.panoramaProjectId),
            "aria-label": "Panorama project",
            onChange: (event) => selectPanoramaProject(event.currentTarget.value),
          },
          h("option", { value: "new" }, "New panorama"),
          projects.map((candidate) =>
            h("option", { key: candidate.id, value: String(candidate.id) }, `${candidate.name} - ${candidate.status}`),
          ),
        ),
        h(
          "button",
          { type: "button", class: "panorama-close", "aria-label": "Close panorama", onClick: closePanoramaWizard },
          "×",
        ),
      ),
    ),
    h(
      "div",
      { class: "panorama-layout" },
      h(
        "section",
        { class: "panorama-sources" },
        h("h3", null, "Sources"),
        h(
          "div",
          { class: "panorama-source-list" },
          images.map((image) => {
            const position = selected.get(image.id);
            const checked = position !== undefined;
            const thumb = image.thumbnail_url || image.preview_url;
            return h(
              "label",
              { key: image.id, class: `panorama-source ${checked ? "selected" : ""}` },
              h("input", {
                type: "checkbox",
                checked,
                disabled: operationRunning,
                onChange: () => togglePanoramaSource(image.id),
              }),
              h("span", { class: "panorama-source-order" }, checked ? String(position + 1) : ""),
              thumb
                ? h("img", {
                    src: versionedUrl(thumb, image.preview_updated_at || image.updated_at),
                    alt: "",
                    loading: "lazy",
                    decoding: "async",
                  })
                : h("span", { class: "panorama-source-placeholder" }),
              h("span", { class: "panorama-source-name", title: image.relative_path }, image.file_name),
              checked
                ? h(
                    "span",
                    { class: "panorama-source-move" },
                    h(
                      "button",
                      {
                        type: "button",
                        title: "Move earlier",
                        "aria-label": `Move ${image.file_name} earlier`,
                        disabled: position === 0 || operationRunning,
                        onClick: (event) => {
                          event.preventDefault();
                          movePanoramaSource(image.id, -1);
                        },
                      },
                      "↑",
                    ),
                    h(
                      "button",
                      {
                        type: "button",
                        title: "Move later",
                        "aria-label": `Move ${image.file_name} later`,
                        disabled: position === state.panoramaImageIds.length - 1 || operationRunning,
                        onClick: (event) => {
                          event.preventDefault();
                          movePanoramaSource(image.id, 1);
                        },
                      },
                      "↓",
                    ),
                  )
                : null,
            );
          }),
        ),
      ),
      h(
        "section",
        { class: "panorama-workflow" },
        h(
          "div",
          { class: "panorama-settings" },
          h(
            "label",
            null,
            h("span", null, "Name"),
            h("input", {
              type: "text",
              value: state.panoramaName,
              disabled: operationRunning,
              autocomplete: "off",
              onInput: (event) => {
                state.panoramaName = event.currentTarget.value;
              },
            }),
          ),
          h(
            "label",
            null,
            h("span", null, "Matching"),
            h(
              "select",
              {
                value: state.panoramaMatching,
                disabled: operationRunning,
                onChange: (event) => {
                  state.panoramaMatching = event.currentTarget.value;
                  renderPanoramaWizard();
                },
              },
              PANORAMA_MATCHING_MODES.map(([value, label]) => h("option", { key: value, value }, label)),
            ),
          ),
          h(
            "button",
            { type: "button", disabled: !canPreview, onClick: generatePanoramaPreviews },
            project?.previews?.length ? "Regenerate previews" : "Generate previews",
          ),
        ),
        h(
          "div",
          { class: "panorama-projections" },
          PANORAMA_PROJECTIONS.map(([value, label]) => {
            const preview = previews.get(value);
            const active = state.panoramaProjection === value;
            return h(
              "button",
              {
                key: value,
                type: "button",
                class: `panorama-projection ${active ? "active" : ""}`,
                disabled: preview?.status !== "done",
                onClick: () => {
                  state.panoramaProjection = value;
                  renderPanoramaWizard();
                },
              },
              h(
                "span",
                { class: "panorama-projection-media" },
                preview?.url
                  ? h("img", {
                      src: versionedUrl(preview.url, preview.updated_at),
                      alt: `${label} panorama preview`,
                      loading: "lazy",
                      decoding: "async",
                    })
                  : h("span", null, preview ? capitalize(preview.status) : "Not rendered"),
              ),
              h("span", { class: "panorama-projection-label" }, label),
            );
          }),
        ),
      ),
    ),
    h(
      "footer",
      { class: "panorama-footer" },
      h(
        "div",
        { class: "panorama-status" },
        operationRunning
          ? h(
              Fragment,
              null,
              h("span", null, panoramaStatusText(project)),
              h("progress", { max: progressTotal, value: progressValue }),
            )
          : h(
              "span",
              { class: project?.error ? "error" : "" },
              state.panoramaMessage || project?.error || panoramaStatusText(project),
            ),
      ),
      h(
        "div",
        { class: "panorama-footer-actions" },
        project?.result_image_id
          ? h(
              "button",
              {
                type: "button",
                onClick: () => {
                  updateSharedUi({ current_image_id: project.result_image_id, min_rating: minRating() }).catch(
                    (error) => console.error(error),
                  );
                  closePanoramaWizard();
                },
              },
              "Open result",
            )
          : null,
        h(
          "button",
          { type: "button", class: "panorama-render", disabled: !canRender, onClick: renderPanoramaFinal },
          "Render full TIFF",
        ),
      ),
    ),
  );
}

function DiffusionOverlay() {
  const image = findImage(state.diffusionImageId);
  const profile = diffusionProfile(image, state.diffusionProfileIndex);
  const settings = normalizeDiffusionSettings(state.diffusionSettings);
  const job = state.diffusionJob;
  const before = diffusionBeforeSource(job);
  const after = diffusionAfterSource(job);
  const mediaStyle = diffusionMediaStyle(job, image, profile);
  const previewContext = diffusionPreviewContext(job);
  const controlsDisabled = state.diffusionSaving;
  const sourceLabel = diffusionSourceLabel(state.diffusionSource);
  return h(
    "section",
    {
      class: "diffusion-card",
      role: "dialog",
      "aria-modal": "true",
      "aria-labelledby": "diffusion-title",
    },
    h(
      "header",
      { class: "diffusion-header" },
      h(
        "div",
        null,
        h("h2", { id: "diffusion-title" }, "Diffusion"),
        h(
          "p",
          null,
          image && profile
            ? `${image.file_name} | ${profileDisplayName(profile)}${sourceLabel ? ` | ${sourceLabel}` : ""}`
            : "Film-like softness and highlight glow",
        ),
      ),
      h(
        "button",
        {
          type: "button",
          class: "diffusion-close",
          "aria-label": "Cancel diffusion changes",
          disabled: controlsDisabled,
          onClick: closeDiffusion,
        },
        "×",
      ),
    ),
    h(
      "main",
      { class: "diffusion-body" },
      h(
        "div",
        { class: "diffusion-workspace" },
        h(DiffusionFullComparison, { before, after, mediaStyle, job }),
        h(DiffusionDetailComparisons, { before, after, previewContext, job }),
      ),
      h(
        "aside",
        { class: "diffusion-control-rail", "aria-label": "Diffusion controls" },
        h(DiffusionControls, { settings, controlsDisabled }),
        h(
          "div",
          {
            class: `diffusion-status ${state.diffusionError ? "error" : ""}`,
            role: "status",
            "aria-live": "polite",
          },
          h("span", null, state.diffusionError || state.diffusionMessage || diffusionStatusText(job)),
          state.diffusionError && state.diffusionErrorKind === "preview" && !state.diffusionSaving
            ? h("button", { type: "button", onClick: requestDiffusionPreview }, "Retry preview")
            : state.diffusionLoading
              ? h("progress", { max: 1 })
              : null,
        ),
        h(
          "p",
          { class: "diffusion-scope-note" },
          "All applies this diffusion setting to the current profile for every existing and future picture.",
        ),
        h(
          "footer",
          { class: "diffusion-footer" },
          h(
            "div",
            { class: "diffusion-reset-actions" },
            h(
              "button",
              {
                type: "button",
                disabled: controlsDisabled,
                title: "Remove the current picture override and inherit this profile's all-picture setting",
                onClick: () => resetDiffusion("current"),
              },
              "Reset current",
            ),
            h(
              "button",
              {
                type: "button",
                disabled: controlsDisabled,
                title: "Remove this profile's setting for all existing and future pictures",
                onClick: () => resetDiffusion("all"),
              },
              "Reset all",
            ),
          ),
          h(
            "div",
            { class: "diffusion-apply-actions" },
            h("button", { type: "button", disabled: controlsDisabled, onClick: closeDiffusion }, "Cancel"),
            h(
              "button",
              {
                type: "button",
                class: "diffusion-apply",
                disabled: controlsDisabled || state.diffusionLoading,
                title: "Apply these settings only to the current picture and profile",
                onClick: () => applyDiffusion("current"),
              },
              "Apply to current",
            ),
            h(
              "button",
              {
                type: "button",
                class: "diffusion-apply",
                disabled: controlsDisabled || state.diffusionLoading,
                title: "Apply to this profile across all existing and future pictures",
                onClick: () => applyDiffusion("all"),
              },
              "Apply to all",
            ),
          ),
        ),
      ),
    ),
  );
}

function DiffusionFullComparison({ before, after, mediaStyle, job }) {
  return h(
    "section",
    { class: "diffusion-comparison", "aria-label": "Diffusion before and after preview" },
    h(
      "figure",
      null,
      before.url
        ? h("img", {
            src: versionedUrl(reviewUrl(before.url), before.updatedAt),
            alt: "Before diffusion",
            style: mediaStyle,
            decoding: "async",
          })
        : h("div", { class: "diffusion-preview-placeholder", style: mediaStyle }, "Preparing source"),
      h("figcaption", null, "Before"),
    ),
    h(
      "figure",
      null,
      after.url
        ? h("img", {
            src: versionedUrl(reviewUrl(after.url), after.updatedAt),
            alt: "After diffusion",
            style: mediaStyle,
            decoding: "async",
          })
        : h(
            "div",
            { class: "diffusion-preview-placeholder", style: mediaStyle },
            state.diffusionError || diffusionStatusText(job),
          ),
      h("figcaption", null, "After"),
    ),
  );
}

function DiffusionDetailComparisons({ before, after, previewContext, job }) {
  return h(
    "section",
    { class: "diffusion-details", "aria-labelledby": "diffusion-details-title" },
    h(
      "header",
      { class: "diffusion-details-header" },
      h("h3", { id: "diffusion-details-title" }, "Detail comparisons"),
      h("p", null, "Automatically selected from the source preview"),
    ),
    h(
      "div",
      {
        class: "diffusion-detail-strip",
        tabIndex: 0,
        "aria-label": "Automatically selected diffusion detail comparisons",
      },
      DIFFUSION_DETAIL_AREAS.map((definition) => {
        const area = previewContext?.areas.find((candidate) => candidate.kind === definition.kind) || null;
        const note =
          definition.kind === "focus" && area
            ? previewContext.focusSource === "center-fallback"
              ? "Center fallback"
              : previewContext.focusSource === "camera-focus"
                ? "Camera focus"
                : ""
            : "";
        return h(
          "article",
          { key: definition.kind, class: "diffusion-detail-card" },
          h("header", null, h("h4", null, definition.label), note ? h("span", null, note) : null),
          h(
            "div",
            { class: "diffusion-detail-pair" },
            h(DiffusionDetailFigure, {
              label: "Before",
              source: before,
              area,
              previewContext,
              placeholder: area ? "Preparing source" : "Detecting area",
              alt: `${definition.label} before diffusion`,
            }),
            h(DiffusionDetailFigure, {
              label: "After",
              source: after,
              area,
              previewContext,
              placeholder: area ? state.diffusionError || diffusionStatusText(job) : "Detecting area",
              alt: `${definition.label} after diffusion`,
            }),
          ),
        );
      }),
    ),
  );
}

function DiffusionDetailFigure({ label, source, area, previewContext, placeholder, alt }) {
  return h(
    "figure",
    null,
    h(
      "div",
      { class: "diffusion-detail-media", style: diffusionDetailFrameStyle(area) },
      source.url && area && previewContext
        ? h("img", {
            src: versionedUrl(reviewUrl(source.url), source.updatedAt),
            alt,
            style: diffusionDetailMediaStyle(area, previewContext),
            decoding: "async",
          })
        : h("span", { class: "diffusion-detail-placeholder" }, placeholder),
    ),
    h("figcaption", null, label),
  );
}

function DiffusionControls({ settings, controlsDisabled }) {
  return h(
    "div",
    { class: "diffusion-controls" },
    h(
      "section",
      { class: "diffusion-control-section", "aria-labelledby": "diffusion-method-title" },
      h("h3", { id: "diffusion-method-title" }, "Method"),
      h(
        "div",
        { class: "diffusion-method-grid", role: "group", "aria-label": "Diffusion method" },
        DIFFUSION_METHODS.map((method) =>
          h(
            "button",
            {
              key: method.id,
              type: "button",
              class: `diffusion-method-tile ${settings.method === method.id ? "active" : ""}`,
              "aria-pressed": String(settings.method === method.id),
              disabled: controlsDisabled,
              onClick: () => setDiffusionSettings({ method: method.id }),
            },
            h("span", { class: "diffusion-tile-title" }, method.label),
            h("span", { class: "diffusion-tile-description" }, method.description),
          ),
        ),
      ),
    ),
    h(
      "section",
      { class: "diffusion-control-section", "aria-labelledby": "diffusion-preset-title" },
      h("h3", { id: "diffusion-preset-title" }, "Strength"),
      h(
        "div",
        { class: "diffusion-preset-grid", role: "group", "aria-label": "Diffusion strength preset" },
        DIFFUSION_PRESETS.map((preset) => {
          const active = diffusionPresetIsActive(preset, settings);
          const presetSettings = diffusionPresetSettings(preset, settings.method);
          return h(
            "button",
            {
              key: preset.id,
              type: "button",
              class: `diffusion-preset-tile ${active ? "active" : ""}`,
              "aria-pressed": String(active),
              disabled: controlsDisabled,
              onClick: () => setDiffusionSettings(presetSettings),
            },
            h("span", { class: "diffusion-tile-title" }, preset.label),
            h("span", { class: "diffusion-tile-description" }, preset.description),
          );
        }),
      ),
    ),
    h(
      "section",
      {
        class: "diffusion-control-section diffusion-parameter-group",
        "aria-labelledby": "diffusion-softening-title",
      },
      h("h3", { id: "diffusion-softening-title" }, "Softening"),
      h(
        "div",
        { class: "diffusion-sliders" },
        h(DiffusionSlider, {
          id: "diffusion-softness",
          label: "Amount",
          value: settings.softness,
          min: 0,
          max: 100,
          step: 1,
          disabled: controlsDisabled,
          onInput: (value) => setDiffusionSettings({ softness: value }),
        }),
        h(DiffusionSlider, {
          id: "diffusion-softness-radius",
          label: "Radius",
          value: settings.softness_radius_percent,
          min: 50,
          max: 400,
          step: 5,
          disabled: controlsDisabled,
          onInput: (value) => setDiffusionSettings({ softness_radius_percent: value }),
        }),
      ),
    ),
    h(
      "section",
      {
        class: "diffusion-control-section diffusion-parameter-group",
        "aria-labelledby": "diffusion-highlights-title",
      },
      h("h3", { id: "diffusion-highlights-title" }, "Highlights"),
      h(
        "div",
        { class: "diffusion-sliders" },
        h(DiffusionSlider, {
          id: "diffusion-highlight-glow",
          label: "Glow",
          value: settings.highlight_glow,
          min: 0,
          max: 100,
          step: 1,
          disabled: controlsDisabled,
          onInput: (value) => setDiffusionSettings({ highlight_glow: value }),
        }),
        h(DiffusionSlider, {
          id: "diffusion-glow-radius",
          label: "Radius",
          value: settings.glow_radius_percent,
          min: 50,
          max: 400,
          step: 5,
          disabled: controlsDisabled,
          onInput: (value) => setDiffusionSettings({ glow_radius_percent: value }),
        }),
        settings.method === "edge-aware-glow"
          ? h(DiffusionSlider, {
              id: "diffusion-highlight-reach",
              label: "Reach",
              value: settings.highlight_reach,
              min: 0,
              max: 100,
              step: 1,
              disabled: controlsDisabled,
              onInput: (value) => setDiffusionSettings({ highlight_reach: value }),
            })
          : null,
      ),
    ),
    h(
      "section",
      {
        class: "diffusion-control-section diffusion-parameter-group diffusion-overall-controls",
        "aria-labelledby": "diffusion-overall-title",
      },
      h("h3", { id: "diffusion-overall-title" }, "Overall"),
      h(
        "div",
        { class: "diffusion-sliders" },
        h(DiffusionSlider, {
          id: "diffusion-intensity",
          label: "Intensity",
          value: settings.intensity_percent,
          min: 25,
          max: 300,
          step: 5,
          disabled: controlsDisabled,
          onInput: (value) => setDiffusionSettings({ intensity_percent: value }),
        }),
      ),
    ),
  );
}

function DiffusionSlider({ id, label, value, min, max, step, disabled, onInput, formatValue = formatPercent }) {
  const formattedValue = formatValue(value);
  return h(
    "label",
    { class: "diffusion-slider", for: id },
    h("span", null, label),
    h("input", {
      id,
      type: "range",
      min: String(min),
      max: String(max),
      step: String(step),
      value: String(value),
      "aria-valuetext": formattedValue,
      disabled,
      onInput: (event) => onInput(Number(event.currentTarget.value)),
    }),
    h("output", { for: id }, formattedValue),
  );
}

function formatPercent(value) {
  return `${value}%`;
}

function SamplerOverlay() {
  const job = state.samplerJob;
  const hierarchy = buildSamplerHierarchy(job?.entries || []);
  const selectedEntry = samplerSelectedEntry(job);
  const completed = Number(job?.completed || 0);
  const total = Number(job?.total || 0);
  const progressMax = Math.max(1, total);
  const progressValue = Math.min(progressMax, completed);
  const sourceStyle = samplerMediaStyle(job);
  return h(
    "section",
    { class: "sampler-card", role: "dialog", "aria-modal": "true", "aria-labelledby": "sampler-title" },
    h(
      "header",
      { class: "sampler-header" },
      h(
        "div",
        null,
        h("h2", { id: "sampler-title" }, "Sampler"),
        h(
          "p",
          null,
          job
            ? `${job.file_name} | ${completed}/${total} | ${job.workers} workers`
            : state.samplerLoading
              ? "Preparing profile catalog"
              : "Profile sampler",
        ),
      ),
      h(
        "button",
        { type: "button", class: "sampler-close", "aria-label": "Close sampler", onClick: closeSampler },
        "×",
      ),
    ),
    job
      ? h(
          "div",
          { class: "sampler-progress" },
          h("progress", { max: progressMax, value: progressValue }),
          h("span", { class: job.error ? "error" : "" }, job.error || samplerStatusText(job)),
        )
      : null,
    state.samplerError ? h("div", { class: "sampler-error" }, state.samplerError) : null,
    job?.source_url
      ? h(
          "section",
          { class: "sampler-comparison", "aria-label": "Sampler comparison" },
          h(
            "figure",
            null,
            h("img", {
              src: reviewUrl(job.source_url),
              alt: "Neutral source",
              style: sourceStyle,
              decoding: "async",
            }),
            h("figcaption", null, "Neutral"),
          ),
          h(
            "figure",
            null,
            h("img", {
              src: reviewUrl(selectedEntry?.thumbnail_url || job.source_url),
              alt: selectedEntry?.name || "Neutral source",
              style: sourceStyle,
              decoding: "async",
            }),
            h("figcaption", null, selectedEntry?.name || "Select a rendered profile"),
          ),
        )
      : null,
    h(
      "div",
      { class: "sampler-sections" },
      hierarchy.sections.map((section) => h(SamplerSection, { key: section.key, section, job })),
    ),
  );
}

function SamplerSection({ section, job }) {
  const expanded = state.samplerExpandedSections.has(section.key);
  const done = section.allEntries.filter((entry) => entry.status === "done").length;
  return h(
    "details",
    {
      class: `sampler-section sampler-section-depth-${Math.min(section.depth, 3)}`,
      open: expanded,
      "data-sampler-section-key": section.key,
      onToggle: (event) => toggleSamplerSection(section.key, event.currentTarget.open),
    },
    h(
      "summary",
      null,
      h("span", null, section.label),
      h("span", { class: "sampler-section-count" }, `${done}/${section.allEntries.length}`),
    ),
    section.entries.length > 0
      ? h(
          "div",
          { class: "sampler-grid" },
          section.entries.map((entry) => h(SamplerTile, { key: entry.key, entry, job })),
        )
      : null,
    section.children.length > 0
      ? h(
          "div",
          { class: "sampler-section-children" },
          section.children.map((child) => h(SamplerSection, { key: child.key, section: child, job })),
        )
      : null,
  );
}

function SamplerTile({ entry, job }) {
  const selected = state.samplerSelectedKey === entry.key;
  const ready = entry.status === "done" && Boolean(entry.thumbnail_url);
  const currentPending = state.samplerPendingSelections.has(`${entry.key}:current`);
  const allPending = state.samplerPendingSelections.has(`${entry.key}:all`);
  const sourceStyle = samplerMediaStyle(job);
  return h(
    "article",
    {
      class: `sampler-tile ${selected ? "selected" : ""} sampler-${entry.status}`,
      "data-sampler-key": entry.key,
    },
    h(
      "button",
      {
        type: "button",
        class: "sampler-thumbnail",
        disabled: !ready,
        title: entry.filename,
        style: sourceStyle,
        onClick: () => selectSamplerEntry(entry.key),
      },
      ready
        ? h("img", {
            src: reviewUrl(entry.thumbnail_url),
            alt: entry.name,
            style: sourceStyle,
            loading: "lazy",
            decoding: "async",
          })
        : h("span", { class: "sampler-thumbnail-placeholder" }, capitalize(entry.status)),
    ),
    h("div", { class: "sampler-tile-name", title: entry.filename }, entry.name),
    entry.error ? h("div", { class: "sampler-tile-error", title: entry.error }, "Failed") : null,
    h(
      "div",
      { class: "sampler-scope", "aria-label": `${entry.name} availability` },
      h(
        "label",
        { title: "Available for the current picture" },
        h("input", {
          type: "checkbox",
          checked: Boolean(entry.current_enabled),
          disabled: !ready || currentPending,
          onChange: (event) => updateSamplerSelection(entry, "current", event.currentTarget.checked),
        }),
        h("span", null, "Current"),
      ),
      h(
        "label",
        {
          title: entry.configured_from_cli
            ? "Command-line profiles remain available to all pictures"
            : "Available for all current and future pictures",
        },
        h("input", {
          type: "checkbox",
          checked: Boolean(entry.all_enabled),
          disabled: !ready || allPending || entry.configured_from_cli,
          onChange: (event) => updateSamplerSelection(entry, "all", event.currentTarget.checked),
        }),
        h("span", null, "All"),
      ),
    ),
  );
}

function samplerMediaStyle(job) {
  const width = Number(job?.source_width);
  const height = Number(job?.source_height);
  return Number.isFinite(width) && width > 0 && Number.isFinite(height) && height > 0
    ? { aspectRatio: `${width} / ${height}` }
    : undefined;
}

function buildSamplerHierarchy(entries) {
  const root = samplerTrieNode("");
  for (const entry of entries) {
    const parts = Array.isArray(entry.parts) ? entry.parts.map((part) => String(part).trim()).filter(Boolean) : [];
    let node = root;
    for (const part of parts.length > 0 ? parts : ["Profiles"]) {
      if (!node.children.has(part)) node.children.set(part, samplerTrieNode(part));
      node = node.children.get(part);
    }
    node.entries.push(entry);
  }
  const entrySections = new Map();
  const sections = samplerTrieChildren(root).map(([part, node]) =>
    buildSamplerSection(node, [part], 0, [], entrySections),
  );
  return { sections, entrySections };
}

function samplerTrieNode(part) {
  return { part, entries: [], children: new Map() };
}

function samplerTrieChildren(node) {
  return Array.from(node.children.entries()).sort(([left], [right]) =>
    left.localeCompare(right, undefined, { numeric: true }),
  );
}

function buildSamplerSection(node, prefix, depth, ancestorKeys, entrySections) {
  const key = prefix.map(encodeURIComponent).join("/");
  const allEntries = collectSamplerTrieEntries(node);
  const flatten = (depth >= 1 || samplerSubtreeDepth(node) <= 2) && !samplerContainsForcedBranch(node);
  let entries;
  let children;
  if (flatten) {
    entries = allEntries;
    children = [];
  } else {
    entries = [...node.entries];
    children = [];
    for (const [part, child] of samplerTrieChildren(node)) {
      if (child.children.size === 0 && child.entries.length > 0) {
        entries.push(...child.entries);
      } else {
        children.push(buildSamplerSection(child, [...prefix, part], depth + 1, [...ancestorKeys, key], entrySections));
      }
    }
  }
  entries = samplerSortEntries(entries);
  const section = {
    key,
    label: prefix.join(" "),
    depth,
    ancestorKeys,
    entries,
    allEntries,
    children,
  };
  for (const entry of entries) entrySections.set(entry.key, section);
  return section;
}

function collectSamplerTrieEntries(node) {
  const entries = [...node.entries];
  for (const child of node.children.values()) entries.push(...collectSamplerTrieEntries(child));
  return samplerSortEntries(entries);
}

function samplerSortEntries(entries) {
  return [...entries].sort((left, right) => left.name.localeCompare(right.name, undefined, { numeric: true }));
}

function samplerSubtreeDepth(node) {
  if (node.children.size === 0) return 0;
  return 1 + Math.max(...Array.from(node.children.values(), samplerSubtreeDepth));
}

function samplerContainsForcedBranch(node) {
  return Array.from(
    node.children,
    ([part, child]) => samplerIsVersionPart(part) || samplerIsFilmSpeedPart(part) || samplerContainsForcedBranch(child),
  ).some(Boolean);
}

function samplerIsVersionPart(part) {
  return /^v\d+$/i.test(part);
}

function samplerIsFilmSpeedPart(part) {
  if (!/^\d+$/.test(part)) return false;
  const speed = Number(part);
  return speed >= 25 && speed <= 12800;
}

function samplerSelectedEntry(job) {
  const entries = job?.entries || [];
  return (
    entries.find((entry) => entry.key === state.samplerSelectedKey && entry.status === "done") ||
    entries.find((entry) => entry.current_enabled && entry.status === "done") ||
    entries.find((entry) => entry.status === "done") ||
    null
  );
}

function samplerStatusText(job) {
  if (job.status === "preparing") return "Preparing neutral TIFF";
  if (job.status === "rendering") return `Rendering profiles${job.failed ? ` | ${job.failed} failed` : ""}`;
  if (job.status === "done") return job.failed ? `Complete | ${job.failed} failed` : "Complete";
  return capitalize(job.status);
}

function capitalize(value) {
  return value ? `${value[0].toUpperCase()}${value.slice(1)}` : "";
}

const els = {
  status: document.getElementById("status"),
  liveDot: document.getElementById("live-dot"),
  list: document.getElementById("image-list"),
  workspace: document.querySelector(".workspace"),
  viewer: document.querySelector(".viewer"),
  panel: document.querySelector(".panel"),
  image: document.getElementById("main-image"),
  focusOverlay: document.getElementById("focus-overlay"),
  gestureFeedback: document.getElementById("gesture-feedback"),
  zoomFull: document.getElementById("zoom-full"),
  zoomLoupe: document.getElementById("zoom-loupe"),
  histogramOverlay: document.getElementById("histogram-overlay"),
  histogramCanvas: document.getElementById("histogram-canvas"),
  histogramEmpty: document.getElementById("histogram-empty"),
  title: document.getElementById("image-title"),
  profileState: document.getElementById("profile-state"),
  profiles: document.getElementById("profiles"),
  controls: document.querySelector(".controls"),
  imageExif: document.getElementById("image-exif"),
  tags: document.getElementById("tags"),
  notes: document.getElementById("notes"),
  retouchGrid: document.getElementById("retouch-grid"),
  cropStage: document.getElementById("crop-stage"),
  cropCanvas: document.getElementById("crop-canvas"),
  cropSourceImage: document.getElementById("crop-source-image"),
  cropCurrentImage: document.getElementById("crop-current-image"),
  cropOverlay: document.getElementById("crop-overlay"),
  cropBox: document.getElementById("crop-box"),
  cropTools: document.getElementById("crop-tools"),
  cropActions: document.getElementById("crop-actions"),
  cropRotation: document.getElementById("crop-rotation"),
  cropRotationValue: document.getElementById("crop-rotation-value"),
  cropRotateLeft: document.getElementById("crop-rotate-left"),
  cropRotateRight: document.getElementById("crop-rotate-right"),
  cropRatio: document.getElementById("crop-ratio"),
  retouchCopy: document.getElementById("retouch-copy"),
  retouchPaste: document.getElementById("retouch-paste"),
  retouchReset: document.getElementById("retouch-reset"),
  retouchExposure: document.getElementById("retouch-exposure"),
  retouchExposureValue: document.getElementById("retouch-exposure-value"),
  retouchContrast: document.getElementById("retouch-contrast"),
  retouchContrastValue: document.getElementById("retouch-contrast-value"),
  retouchHighlights: document.getElementById("retouch-highlights"),
  retouchHighlightsValue: document.getElementById("retouch-highlights-value"),
  retouchShadows: document.getElementById("retouch-shadows"),
  retouchShadowsValue: document.getElementById("retouch-shadows-value"),
  retouchWhites: document.getElementById("retouch-whites"),
  retouchWhitesValue: document.getElementById("retouch-whites-value"),
  retouchBlacks: document.getElementById("retouch-blacks"),
  retouchBlacksValue: document.getElementById("retouch-blacks-value"),
  retouchTemperature: document.getElementById("retouch-temperature"),
  retouchTemperatureLabel: document.getElementById("retouch-temperature").previousElementSibling,
  retouchTemperatureValue: document.getElementById("retouch-temperature-value"),
  retouchOffset: document.getElementById("retouch-offset"),
  retouchOffsetValue: document.getElementById("retouch-offset-value"),
  retouchClarity: document.getElementById("retouch-clarity"),
  retouchClarityValue: document.getElementById("retouch-clarity-value"),
  cropToggle: document.getElementById("crop-toggle"),
  cropOk: document.getElementById("crop-ok"),
  cropCancel: document.getElementById("crop-cancel"),
  cropReset: document.getElementById("crop-reset"),
  publish: document.getElementById("publish"),
  diffusion: document.getElementById("diffusion"),
  sampler: document.getElementById("sampler"),
  panorama: document.getElementById("panorama"),
  minRating: document.getElementById("min-rating"),
  app: document.querySelector(".app"),
  shortcutsHelp: document.getElementById("shortcuts-help"),
  mobileDrawerButtons: document.querySelectorAll("[data-mobile-drawer]"),
  mobileSaveOriginal: document.getElementById("mobile-save-original"),
  mobilePublish: document.getElementById("mobile-publish"),
  shortcutsOverlay: document.getElementById("shortcuts-overlay"),
  shortcutsClose: document.getElementById("shortcuts-close"),
  profileInfoOverlay: document.getElementById("profile-info-overlay"),
  commandInvocationOverlay: document.getElementById("command-invocation-overlay"),
  appVersion: document.getElementById("app-version"),
  publishOverlay: document.getElementById("publish-overlay"),
  publishForm: document.getElementById("publish-form"),
  publishCancel: document.getElementById("publish-cancel"),
  publishSubmit: document.getElementById("publish-submit"),
  publishStatus: document.getElementById("publish-status"),
  publishMode: document.getElementById("publish-mode"),
  publishCount: document.getElementById("publish-count"),
  publishAlbum: document.getElementById("publish-album"),
  publishMinRating: document.getElementById("publish-min-rating"),
  publishTags: document.getElementById("publish-tags"),
  publishOutputFormat: document.getElementById("publish-output-format"),
  publishGrainEngine: document.getElementById("publish-grain-engine"),
  publishNormalizeGrain: document.getElementById("publish-normalize-grain"),
  publishNormalizeGrainMpix: document.getElementById("publish-normalize-grain-mpix"),
  publishSizeMode: document.getElementById("publish-size-mode"),
  publishLongEdge: document.getElementById("publish-long-edge"),
  publishMaxWidth: document.getElementById("publish-max-width"),
  publishMaxHeight: document.getElementById("publish-max-height"),
  publishResize: document.getElementById("publish-resize"),
  publishJpgQuality: document.getElementById("publish-jpg-quality"),
  publishJpegSubsampling: document.getElementById("publish-jpeg-subsampling"),
  publishProgressive: document.getElementById("publish-progressive"),
  publishStripMetadata: document.getElementById("publish-strip-metadata"),
  publishGallery: document.getElementById("publish-gallery"),
  publishGalleryColumns: document.getElementById("publish-gallery-columns"),
  publishGalleryThumbnailLongEdge: document.getElementById("publish-gallery-thumbnail-long-edge"),
  panoramaOverlay: document.getElementById("panorama-overlay"),
  samplerOverlay: document.getElementById("sampler-overlay"),
  diffusionOverlay: document.getElementById("diffusion-overlay"),
};

const wideProfilesQuery = window.matchMedia("(min-width: 901px) and (min-height: 620px)");
const mobileReviewQuery = window.matchMedia("(max-width: 600px), (max-width: 950px) and (max-height: 520px)");

function reviewUrl(path) {
  return path.replace(/^\/+/, "");
}

async function loadState() {
  const response = await fetch(reviewUrl("api/state"), { cache: "no-store" });
  if (!response.ok) throw new Error(`state ${response.status}`);
  applyState(await response.json());
}

function applyStateMessage(message) {
  if (message?.type === "patch") {
    applyStatePatch(message);
  } else {
    applyState(message);
  }
}

function applyState(data) {
  if (state.data?.version && data?.version && state.data.version !== data.version) {
    window.location.reload();
    return;
  }
  mergeIncomingProfileSelections(data);
  state.data = data;
  state.localRetouchDirty = false;
  applyServerUi(data);
  render();
}

function applyStatePatch(patch) {
  if (!state.data) {
    loadState().catch((error) => {
      els.status.textContent = `Disconnected: ${error.message}`;
    });
    return;
  }
  if (state.data?.version && patch?.version && state.data.version !== patch.version) {
    window.location.reload();
    return;
  }
  const data = { ...state.data };
  for (const key of [
    "profiles",
    "client_count",
    "codex",
    "publish_defaults",
    "publish_jobs",
    "capabilities",
    "panorama",
    "bursts",
    "ui",
    "publish_root",
    "invocation",
  ]) {
    if (Object.prototype.hasOwnProperty.call(patch, key)) data[key] = patch[key];
  }
  if (Array.isArray(patch.images) || Array.isArray(patch.removed_image_ids)) {
    const byId = new Map((data.images || []).map((image) => [image.id, image]));
    for (const id of patch.removed_image_ids || []) byId.delete(id);
    for (const image of patch.images || []) byId.set(image.id, image);
    if (Array.isArray(patch.image_ids)) {
      data.images = patch.image_ids.map((id) => byId.get(id)).filter(Boolean);
    } else {
      data.images = Array.from(byId.values());
    }
  }
  applyState(data);
}

function mergeIncomingProfileSelections(data) {
  for (const image of data?.images || []) {
    const current = findImage(image.id);
    const pending = state.pendingProfileSelections.get(image.id);
    if (pending !== undefined) {
      if (image.selected_profile_index === pending) {
        state.pendingProfileSelections.delete(image.id);
      } else {
        image.selected_profile_index = pending;
      }
      continue;
    }
    if (
      current &&
      current.selected_profile_index !== image.selected_profile_index &&
      incomingImageIsOlder(image, current)
    )
      image.selected_profile_index = current.selected_profile_index;
  }
}

function incomingImageIsOlder(incoming, current) {
  const incomingTime = String(incoming?.updated_at || "");
  const currentTime = String(current?.updated_at || "");
  return incomingTime.length > 0 && currentTime.length > 0 && incomingTime < currentTime;
}

function applyServerUi(data) {
  els.minRating.value = String(Math.max(0, Math.min(5, Number(data?.ui?.min_rating) || 0)));
  state.currentId = data?.ui?.current_image_id ?? firstReviewableImageIdFromData(data);
}

function firstReviewableImageId() {
  return firstReviewableImageIdFromData(state.data);
}

function firstReviewableImageIdFromData(data) {
  const images = filteredImagesFromData(data);
  return images.length > 0 ? images[0].id : null;
}

function findImage(id) {
  return findImageInData(state.data, id);
}

function findImageInData(data, id) {
  return (data?.images || []).find((image) => image.id === id) || null;
}

function render() {
  if (state.profileInfoProfileIndex !== null && !profileByIndex(state.profileInfoProfileIndex)) {
    state.profileInfoProfileIndex = null;
    clearProfileInfoPp3();
  }
  renderProfileInfo();
  renderCommandInvocation();
  renderPanoramaWizard();
  renderSampler();
  renderDiffusion();
  const panoramaAvailable = Boolean(state.data?.capabilities?.panorama?.available);
  els.panorama.hidden = !panoramaAvailable;
  els.sampler.hidden = !state.data?.capabilities?.sampler;
  els.sampler.disabled = !findImage(state.currentId);
  els.diffusion.disabled = !currentDiffusionContext();
  syncProfilesPlacement();
  const images = filteredImages();
  const total = state.data?.images?.length || 0;
  const profileCount = visibleProfileCount();
  const clientCount = state.data?.client_count || 0;
  const publishSummary = latestPublishJobSummary();
  const codexSummary = codexSummaryText();
  els.appVersion.textContent = `mini-film ${state.data?.version || ""}`.trim();
  els.status.textContent = `${images.length}/${total} pictures | ${profileCount} ${plural(profileCount, "profile")} | ${clientCount} ${plural(clientCount, "client")}${codexSummary ? ` | ${codexSummary}` : ""}${publishSummary ? ` | ${publishSummary}` : ""}`;
  updatePublishStatus();
  let current = findImage(state.currentId);
  if (current && !passesFilter(current)) current = null;
  if (!current) {
    state.currentId = firstReviewableImageId();
    current = findImage(state.currentId);
  }
  renderList(images);
  renderCurrent(current);
  scheduleViewerSafeAreaUpdate();
}

function plural(count, singular) {
  return Number(count) === 1 ? singular : `${singular}s`;
}

function latestPublishJob() {
  const jobs = state.data?.publish_jobs || [];
  return jobs.length > 0 ? jobs[jobs.length - 1] : null;
}

function latestPublishJobSummary() {
  const job = latestPublishJob();
  if (!job) return "";
  if (job.status === "running") {
    const percent = publishProgressPercent(job);
    const current = job.current ? ` | ${job.current}` : "";
    return `publishing ${job.album} ${percent}% ${job.step || "publish"}${current}`;
  }
  if (job.status === "done") return `published ${job.linked} files`;
  return `publish failed`;
}

function codexSummaryText() {
  const codex = state.data?.codex;
  if (!codex?.enabled) return "";
  const processing = Number(codex.processing || 0);
  const queued = Number(codex.queued || 0);
  const failed = Number(codex.failed || 0);
  if (processing > 0) return `codex analyzing ${processing}${queued > 0 ? ` queued ${queued}` : ""}`;
  if (queued > 0) return `codex queued ${queued}`;
  if (failed > 0) return `codex failed ${failed}`;
  return "";
}

function publishProgressPercent(job) {
  const total = Number(job?.total || 0);
  const processed = Number(job?.processed || 0);
  if (total <= 0) return job?.status === "done" ? 100 : 0;
  return Math.max(0, Math.min(100, Math.round((processed / total) * 100)));
}

function PublishStatus({ job }) {
  if (job.status === "running") {
    const percent = publishProgressPercent(job);
    return h(
      "div",
      null,
      h(
        "div",
        null,
        `Publishing ${job.album}: ${percent}% ${job.step || "publish"}${job.current ? ` | ${job.current}` : ""}`,
      ),
      h("div", { class: "publish-progress" }, h("span", { style: { width: `${percent}%` } })),
      h(
        "div",
        { class: "publish-progress-counts" },
        `${job.processed || 0}/${job.total || 0} outputs | linked ${job.linked || 0} | skipped ${job.skipped || 0} | galleries ${job.galleries || 0}`,
      ),
    );
  }
  if (job.status === "done") {
    const links = Array.isArray(job.gallery_urls) ? job.gallery_urls : [];
    const galleryLinks = links.map((link) => (link.startsWith("/") ? link : `/${link}`));
    return h(
      "div",
      null,
      `Published ${job.linked} files to ${job.album}; skipped ${job.skipped}; galleries ${job.galleries}.`,
      galleryLinks.length
        ? h(
            "div",
            { class: "publish-galleries" },
            h("div", null, "Gallery links:"),
            h(
              "div",
              { class: "publish-gallery-list" },
              galleryLinks.map((link, index) =>
                h(
                  "div",
                  { class: "publish-gallery-row" },
                  h(
                    "a",
                    {
                      href: link,
                      target: "_blank",
                      rel: "noopener noreferrer",
                      class: "publish-gallery-link",
                    },
                    galleryLinks.length > 1 ? `Gallery ${index + 1}` : "Open gallery",
                  ),
                  h(
                    "a",
                    {
                      href: reviewUrl(`api/publish/${job.id}/gallery.zip`),
                      download: "",
                      class: "publish-gallery-link",
                    },
                    "Download gallery",
                  ),
                ),
              ),
            ),
          )
        : null,
    );
  }
  return `Publish failed: ${job.error || "unknown error"}`;
}

function updatePublishStatus() {
  const job = latestPublishJob();
  if (!job) {
    els.publishSubmit.disabled = false;
    preactRender(
      publishWouldRerender()
        ? "Changed output settings will rerender from original RAWs."
        : "Default settings will link existing reviewed outputs.",
      els.publishStatus,
    );
    return;
  }
  if (job.status === "running") {
    els.publishSubmit.disabled = true;
  } else if (job.status === "done") {
    els.publishSubmit.disabled = false;
  } else {
    els.publishSubmit.disabled = false;
  }
  preactRender(h(PublishStatus, { job }), els.publishStatus);
}

function syncProfilesPlacement() {
  const shouldUseRail = wideProfilesQuery.matches;
  const parent = els.profiles.parentElement;
  if (shouldUseRail && parent !== els.workspace) {
    els.workspace.append(els.profiles);
    scheduleViewerSafeAreaUpdate();
    return;
  }
  if (!shouldUseRail && parent !== els.panel) {
    els.panel.insertBefore(els.profiles, els.controls);
    scheduleViewerSafeAreaUpdate();
  }
}

function updateViewerSafeArea() {
  const workspaceRect = els.workspace.getBoundingClientRect();
  const panelRect = els.panel.getBoundingClientRect();
  const panelSafe = Math.max(0, Math.ceil(workspaceRect.bottom - panelRect.top));

  els.workspace.style.setProperty("--review-panel-safe", `${panelSafe}px`);
  if (!els.retouchGrid.hidden) positionRetouchGrid();
  if (!els.focusOverlay.hasAttribute("hidden")) positionFocusOverlay();
  if (!els.cropOverlay.hidden) positionCropOverlay();
  if (!els.gestureFeedback.hidden) positionGestureFeedback();
  if (state.zoomFullActive && state.zoomFullLastPoint) {
    updateFullImageZoom(state.zoomFullLastPoint.clientX, state.zoomFullLastPoint.clientY);
  } else if (state.zoomActive && state.zoomLastPoint) {
    updateZoomLoupe(state.zoomLastPoint.clientX, state.zoomLastPoint.clientY);
  }
}

let viewerSafeAreaFrame = 0;
function scheduleViewerSafeAreaUpdate() {
  if (viewerSafeAreaFrame) return;
  viewerSafeAreaFrame = requestAnimationFrame(() => {
    viewerSafeAreaFrame = 0;
    updateViewerSafeArea();
  });
}

function minRating() {
  return Number(els.minRating.value || 0);
}

function passesFilter(image) {
  return Number(image.rating || 0) >= minRating();
}

function filteredImages() {
  return filteredImagesFromData(state.data);
}

function filteredImagesFromData(data) {
  return (data?.images || []).filter(passesFilter);
}

function renderList(images) {
  let lastCaptureDay = null;
  const displayImages = images.map((image) => {
    const captureDay = imageCaptureDisplay(image, lastCaptureDay);
    lastCaptureDay = captureDay.day;
    return { ...image, capture_time: captureDay.text };
  });
  preactRender(
    h(ImageList, {
      images: displayImages,
      bursts: state.data?.bursts || [],
      currentId: state.currentId,
      onSelect: async (image) => {
        const carryProfileIndex = selectedProfile(findImage(state.currentId))?.profile_index;
        await saveCurrentIfNeeded();
        await updateSharedUi({ current_image_id: image.id, min_rating: minRating() });
        await carrySelectedProfileToImage(image.id, carryProfileIndex);
      },
      onToggleBurst: updateBurstExpansion,
    }),
    els.list,
  );
  const activeRow = els.list.querySelector(".burst-member.active") || els.list.querySelector(".image-row.active");
  if (activeRow) {
    requestAnimationFrame(() => {
      activeRow.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
  }
}

function ImageList({ images, bursts, currentId, onSelect, onToggleBurst }) {
  const imageById = new Map(images.map((image) => [String(image.id), image]));
  const burstByImageId = new Map();

  for (const burst of Array.isArray(bursts) ? bursts : []) {
    if (burst?.id === undefined || burst?.id === null || !Array.isArray(burst.image_ids)) continue;
    const memberIds = Array.from(new Set(burst.image_ids.map(String)));
    const members = memberIds
      .filter((imageId) => !burstByImageId.has(imageId))
      .map((imageId) => imageById.get(imageId))
      .filter(Boolean);
    if (members.length < 2) continue;
    const visibleBurst = {
      ...burst,
      members,
      total: memberIds.length,
    };
    for (const member of members) burstByImageId.set(String(member.id), visibleBurst);
  }

  const renderedBursts = new Set();
  return images.flatMap((image) => {
    const burst = burstByImageId.get(String(image.id));
    if (!burst) {
      return [h(ImageRow, { key: `image:${image.id}`, image, currentId, onSelect })];
    }

    const burstKey = String(burst.id);
    if (renderedBursts.has(burstKey)) return [];
    renderedBursts.add(burstKey);
    return [
      h(BurstGroup, {
        key: `burst:${burstKey}`,
        burst,
        currentId,
        onSelect,
        onToggleBurst,
      }),
    ];
  });
}

function BurstGroup({ burst, currentId, onSelect, onToggleBurst }) {
  const currentMember = burst.members.find((image) => image.id === currentId);
  const displayed = currentMember || burst.members[0];
  const visibleCount = burst.members.length;
  const count = `${visibleCount}/${burst.total}`;
  const expanded = Boolean(burst.expanded);
  const expansionLabel = `${expanded ? "Collapse" : "Expand"} burst, ${visibleCount} of ${burst.total} ${plural(
    burst.total,
    "picture",
  )} visible`;

  return h(
    "section",
    {
      class: `burst-group${currentMember ? " contains-active" : ""}${expanded ? " expanded" : ""}`,
      role: "group",
      "aria-label": `Burst, ${visibleCount} of ${burst.total} ${plural(burst.total, "picture")} visible`,
    },
    h(
      "div",
      { class: "burst-header" },
      h(ImageRow, {
        image: displayed,
        currentId: expanded ? null : currentId,
        onSelect,
        className: "burst-summary",
        burstCount: count,
        isolateActivation: true,
      }),
      h(
        "button",
        {
          type: "button",
          class: "burst-toggle",
          title: expansionLabel,
          "aria-label": expansionLabel,
          "aria-expanded": String(expanded),
          onKeyDown: isolateBurstActivation,
          onClick: (event) => {
            event.preventDefault();
            event.stopPropagation();
            onToggleBurst(burst.id, !expanded).catch((error) => console.error(error));
          },
        },
        h("span", { class: "burst-chevron-icon", "aria-hidden": "true" }, "\u203a"),
      ),
    ),
    expanded
      ? h(
          "div",
          { class: "burst-members" },
          burst.members.map((image) =>
            h(ImageRow, {
              key: `burst:${burst.id}:image:${image.id}`,
              image,
              currentId,
              onSelect,
              className: "burst-member",
            }),
          ),
        )
      : null,
  );
}

function isolateBurstActivation(event) {
  if (event.key === "Enter" || event.key === " ") event.stopPropagation();
}

function ImageRow({ image, currentId, onSelect, className = "", burstCount = null, isolateActivation = false }) {
  const progress = renderProgressSummary(image);
  const labels = imageLabels(image);
  const isActive = image.id === currentId;
  const thumbnailUrl = image.thumbnail_url || image.preview_url;
  return h(
    "button",
    {
      type: "button",
      class: `image-row${className ? ` ${className}` : ""}${isActive ? " active" : ""}`,
      "aria-current": isActive ? "true" : undefined,
      onKeyDown: isolateActivation ? isolateBurstActivation : undefined,
      onClick: () => onSelect(image).catch((error) => console.error(error)),
    },
    h("img", {
      class: "image-row-thumb",
      alt: "",
      src: thumbnailUrl ? versionedUrl(thumbnailUrl, image.preview_updated_at || image.updated_at) : undefined,
      loading: "lazy",
      decoding: "async",
      fetchpriority: "low",
    }),
    h(
      "div",
      {
        class: "image-row-title",
        title: image.relative_path || image.file_name,
      },
      h("span", { class: "image-row-title-text" }, image.file_name),
      burstCount
        ? h("span", { class: "burst-count", title: `${burstCount} burst pictures visible` }, burstCount)
        : null,
    ),
    image.capture_time
      ? h("span", { class: "image-row-capture-time", title: image.capture_time }, image.capture_time)
      : null,
    h(
      "div",
      { class: "image-row-meta" },
      h("span", { class: "image-row-rating" }, image.rating, labels.length > 0 ? h(LabelBadges, { labels }) : null),
      h(
        "span",
        {
          class: "image-row-progress",
          title: progress.title,
        },
        progress.text,
      ),
    ),
    h("span", {
      class: `image-row-indicator ${progress.state}`,
      title: progress.title,
      "aria-label": progress.title,
    }),
  );
}

function renderProgressSummary(image) {
  if (isDirectCompressedImage(image)) {
    if (isLocalRetouchDraft(image)) {
      return {
        state: "retouch-draft",
        text: "crop draft",
        title: "crop draft preview is local; server render will queue after edits settle",
      };
    }
    const codexState = codexProgressState(image);
    if (codexState) return codexState;
    const display = compressedDisplayState(image);
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

  if (isLocalRetouchDraft(image)) {
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
      title: profilesAreImplicitOnly(image) ? "RawTherapee default render pending" : "no profiles selected for publish",
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

function codexProgressState(image) {
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
      title: `Codex image analysis failed${image.codex.error ? `: ${image.codex.error}` : ""}`,
    };
  }
  return null;
}

function renderCurrent(image) {
  updateDiffusionButton(image);
  if (!image) {
    stopZoom();
    clearCropDraftState();
    els.viewer.classList.remove("has-image");
    els.app.classList.remove("compressed-image");
    els.app.classList.remove("sooc-profile-selected");
    setRetouchControlsEnabled(false);
    els.image.removeAttribute("src");
    els.title.textContent = "";
    els.title.removeAttribute("title");
    els.profileState.textContent = "";
    els.imageExif.replaceChildren();
    preactRender(null, els.profiles);
    els.tags.value = "";
    els.notes.value = "";
    if (state.histogramOpen) showHistogramEmpty("No image");
    renderFocusOverlay(null);
    state.lastInputImageId = null;
    setActiveReviewButtons(null);
    updateMobileActionLabels(null);
    return;
  }

  const selected = selectedProfile(image);
  const directCompressed = isDirectCompressedImage(image);
  els.app.classList.toggle("compressed-image", directCompressed);
  els.app.classList.toggle("sooc-profile-selected", isSoocProfile(selected));
  const hideProfiles = profilesAreImplicitOnly(image) || directCompressed;
  const mainSource = mainImageSource(image, selected);
  const mainUrl = mainSource.url;
  const previewNote = selected?.url || directCompressed ? "" : image.preview_url ? " | camera preview" : "";
  const selectedState = directCompressed ? compressedDisplayState(image) : profileDisplayState(image, selected);
  const codexState = currentCodexStateText(image);
  if (state.cropDraftImageId !== null && state.cropDraftImageId !== image.id) {
    clearCropDraftState();
  }
  els.title.textContent = image.file_name;
  els.title.title = imageSourceInfoTitle(image);
  renderImageExif(image);
  renderProfileStateSummary(image, selected, selectedState, previewNote, codexState, hideProfiles);
  const imageChanged = state.lastInputImageId !== image.id;
  if (imageChanged || document.activeElement !== els.tags) {
    els.tags.value = image.tags.join(", ");
  }
  if (imageChanged || document.activeElement !== els.notes) {
    els.notes.value = image.notes || "";
  }
  if (imageChanged || !isRetouchControlActive()) {
    setRetouchInputs(retouchForImage(image, image.retouch || defaultRetouch()), image);
  }
  setRetouchControlsEnabled(!directCompressed && !isSoocProfile(selected));
  state.lastInputImageId = image.id;
  setActiveReviewButtons(image);

  if (mainUrl) {
    els.viewer.classList.add("has-image");
    const nextSrc = versionedUrl(mainUrl, mainSource.updatedAt);
    const sourceChanged = els.image.getAttribute("src") !== nextSrc;
    if (imageChanged || (sourceChanged && !state.zoomFullActive)) {
      stopZoom();
    } else if (sourceChanged) {
      clearZoomSource();
    }
    els.image.src = nextSrc;
    els.image.alt = image.file_name;
  } else {
    stopZoom();
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
  }

  applyDraftRetouch(image, selected);
  renderFocusOverlay(image);
  scheduleHistogramRender();
  renderRetouchGrid(image, selected);
  renderCropOverlay(image);
  renderProfiles(image);
  updateMobileActionLabels(image);
  const cropSource = cropEditingSource(image, selected);
  if (cropSource.url && cropSource.url !== mainUrl) {
    preloadImage(versionedUrl(cropSource.url, cropSource.updatedAt));
  }
  preloadNearbyImages(image);
}

function currentCodexStateText(image) {
  const status = image?.codex?.status;
  if (status === "processing") return "Codex analyzing";
  if (status === "queued") return "Codex queued";
  if (status === "failed") return "Codex failed";
  return "";
}

function renderProfileStateSummary(image, selected, selectedState, previewNote, codexState, hideProfiles) {
  const selectedName = !hideProfiles && selected ? profileDisplayName(selected) : "";
  const dcpFilename = typeof selected?.dcp_profile_filename === "string" ? selected.dcp_profile_filename.trim() : "";
  const lcpFilename = typeof selected?.lcp_profile_filename === "string" ? selected.lcp_profile_filename.trim() : "";
  if (isDirectCompressedImage(image) || !selectedName) {
    els.profileState.textContent = `${selectedState?.text || ""}${codexState ? ` | ${codexState}` : ""}`.trim();
    return;
  }
  const suffix = `${selectedState?.text || ""}${previewNote || ""}${codexState ? ` | ${codexState}` : ""}`;
  preactRender(
    h(
      "span",
      { class: "profile-state-summary" },
      h(
        "button",
        {
          type: "button",
          class: "current-profile-link",
          onClick: () => openProfileInfo(selected),
        },
        selectedName,
      ),
      dcpFilename || lcpFilename
        ? h(
            "span",
            {
              title: [dcpFilename && `DCP: ${dcpFilename}`, lcpFilename && `LCP: ${lcpFilename}`]
                .filter(Boolean)
                .join("; "),
              "aria-label":
                dcpFilename && lcpFilename
                  ? `DCP + LCP used: DCP: ${dcpFilename}; LCP: ${lcpFilename}`
                  : dcpFilename
                    ? `DCP used: ${dcpFilename}`
                    : `LCP used: ${lcpFilename}`,
            },
            dcpFilename && lcpFilename ? "DCP + LCP used" : dcpFilename ? "DCP used" : "LCP used",
          )
        : null,
      selected.bw_filter_eligible ? h(BwFilterControls, { image, profile: selected }) : null,
      `: ${suffix}`,
    ),
    els.profileState,
  );
}

function BwFilterControls({ image, profile }) {
  const active = normalizeBwFilter(profile.bw_filter);
  return h(
    "span",
    { class: "bw-filter-controls", role: "group", "aria-label": "Black-and-white filter" },
    BW_FILTERS.map((filter) =>
      h(
        "button",
        {
          key: filter,
          type: "button",
          class: normalizeBwFilter(filter) === active ? "active" : "",
          title: `${BW_FILTER_NAMES.get(filter)} black-and-white filter`,
          "aria-label": filter === "none" ? "No black-and-white filter" : `${filter} black-and-white filter`,
          "aria-pressed": normalizeBwFilter(filter) === active ? "true" : "false",
          onClick: (event) => {
            event.preventDefault();
            event.stopPropagation();
            if (normalizeBwFilter(filter) === active) return;
            setProfileBwFilter(image, profile.profile_index, filter).catch((error) => console.error(error));
          },
        },
        BW_FILTER_LABELS.get(filter),
      ),
    ),
  );
}

function imageCaptureDisplay(image, previousDay) {
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

function zeroPad(value) {
  return String(value).padStart(2, "0");
}

function renderImageExif(image) {
  els.imageExif.replaceChildren();
  els.imageExif.removeAttribute("title");
  const exif = image?.exif || {};
  const exposureCompensation = formatExposureCompensation(exif.exposure_compensation);
  const shutterCountTitle =
    exif.shutter_count === null || exif.shutter_count === undefined || exif.shutter_count === ""
      ? ""
      : `Shutter count: ${exif.shutter_count}`;
  const isoTitle = exif.auto_iso ? (exif.iso_auto_hi_limit ? `Auto ISO <= ${exif.iso_auto_hi_limit}` : "Auto ISO") : "";
  const lensTitle = exif.lens_model ? `Lens: ${exif.lens_model}` : "";
  const releaseModeTitle = exif.release_mode ? `Release mode: ${exif.release_mode}` : "";
  const shutterDetailsTitle = [
    exif.shutter_mode ? `Shutter mode: ${exif.shutter_mode}` : "",
    exif.silent_photography ? "Silent photography: On" : "",
  ]
    .filter(Boolean)
    .join(" · ");
  const parts = [
    { text: exif.shooting_mode ? `Mode ${exif.shooting_mode}` : "", title: releaseModeTitle },
    { text: exif.camera_model || "", className: "image-exif-camera", title: shutterCountTitle },
    { text: formatExifFocalLength(exif.focal_length), title: lensTitle },
    { text: exif.iso ? `ISO ${exif.iso}` : "", title: isoTitle },
    { text: formatExifAperture(exif.aperture) },
    { text: exif.shutter_speed || "", title: shutterDetailsTitle },
    { text: exposureCompensation },
    { text: exif.flash ? `Flash ${exif.flash}` : "" },
  ].filter((part) => part.text);
  const text = parts.map((part) => part.text).join(" · ");
  parts.forEach((part, index) => {
    if (index > 0) els.imageExif.append(document.createTextNode(" · "));
    const span = document.createElement("span");
    span.textContent = part.text;
    if (part.className) span.className = part.className;
    span.title = part.title || text;
    els.imageExif.append(span);
  });
}

function formatExposureCompensation(value) {
  if (!value && value !== 0) return "";
  const number = Number(value);
  if (!Number.isFinite(number) || number === 0) return "";
  const normalized = Number(number.toFixed(1));
  if (normalized === 0) return "";
  return `${normalized > 0 ? "+" : ""}${normalized.toFixed(1)}EV`;
}

function formatExifFocalLength(value) {
  return formatExifNumberText(value, 2);
}

function formatExifAperture(value) {
  return formatExifNumberText(value, 1);
}

function formatExifNumberText(value, maxDigits) {
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

function selectedProfile(image) {
  return selectedProfileForImage(image);
}

function selectedProfileForImage(image) {
  if (isDirectCompressedImage(image)) return null;
  const profiles = (image?.profiles || []).filter((profile) => isSoocProfile(profile) || profile.enabled !== false);
  const selectedIndex = selectedProfileIndexForImage(image);
  const selected = profiles.find((profile) => profile.profile_index === selectedIndex);
  return selected || profiles[0] || null;
}

function setRetouchControlsEnabled(enabled) {
  const controls = [
    els.retouchCopy,
    els.retouchPaste,
    els.retouchReset,
    els.retouchExposure,
    els.retouchContrast,
    els.retouchHighlights,
    els.retouchShadows,
    els.retouchWhites,
    els.retouchBlacks,
    els.retouchTemperature,
    els.retouchOffset,
    els.retouchClarity,
    els.cropToggle,
    els.cropOk,
    els.cropCancel,
    els.cropReset,
  ];
  controls.forEach((control) => {
    if (control) control.disabled = !enabled;
  });
  document.querySelectorAll(".retouch label > span").forEach((label) => {
    label.classList.toggle("retouch-adjustment-label-disabled", !enabled);
  });
  syncRetouchClipboardButtons();
}

function selectedProfileIndexForImage(image) {
  if (!image) return undefined;
  return state.pendingProfileSelections.get(image.id) ?? image.selected_profile_index;
}

function isCompressedImage(image) {
  return image?.source_type === "compressed";
}

function usesProfilePipeline(image) {
  if (!image) return false;
  if (image.processing_mode) return image.processing_mode === "profiled";
  return !isCompressedImage(image);
}

function isDirectCompressedImage(image) {
  return isCompressedImage(image) && !usesProfilePipeline(image);
}

function imageSourceInfoTitle(image) {
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

function formatFileSize(bytes) {
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

function compressedViewportUsesFullMedia() {
  return Math.max(window.innerWidth, window.innerHeight) > COMPRESSED_REVIEW_PREVIEW_LONG_EDGE;
}

function mainImageSource(image, selected = selectedProfile(image)) {
  if (selected?.url) return { url: selected.url, updatedAt: selected.updated_at };
  if (isDirectCompressedImage(image) && image?.full_url && compressedViewportUsesFullMedia()) {
    return { url: image.full_url, updatedAt: image.preview_updated_at || image.updated_at };
  }
  return { url: image?.preview_url, updatedAt: image?.preview_updated_at || image?.updated_at };
}

function syncMainImageForViewport() {
  const image = findImage(state.currentId);
  if (!isDirectCompressedImage(image)) return;
  const source = mainImageSource(image, null);
  if (!source.url) return;
  const nextSrc = versionedUrl(source.url, source.updatedAt);
  if (els.image.getAttribute("src") === nextSrc) return;
  stopZoom();
  els.image.src = nextSrc;
  preloadNearbyImages(image);
}

function isSoocProfile(profile) {
  return profile?.profile_stem === "sooc" || profile?.profile_index === 1000000000;
}

function isRetouchControlsDisabledForImage(image) {
  if (!image) return true;
  return isDirectCompressedImage(image) || isSoocProfile(selectedProfile(image));
}

function isPortraitRenderProfile(profile) {
  const width = Number(profile?.width);
  const height = Number(profile?.height);
  return Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0 && height > width;
}

function hasSoocProfile(image) {
  return Boolean((image?.profiles || []).some((profile) => isSoocProfile(profile)));
}

function profileDisplayName(profile) {
  return profile?.display_name || profile?.profile_stem || "profile";
}

function isLocalRetouchDraft(image) {
  return Boolean(image && image.id === state.currentId && state.localRetouchDirty);
}

function compressedDisplayState(image) {
  if (!image) {
    return {
      state: "waiting",
      text: "waiting",
      title: "waiting for image render",
    };
  }
  if (isLocalRetouchDraft(image)) {
    return {
      state: "retouch-draft",
      text: "crop draft",
      title: "local crop preview; server render will queue after edits settle",
    };
  }
  if (image.preview_retouch_pending && image.preview_status === "processing") {
    return {
      state: "retouch-processing",
      text: "crop rendering",
      title: "server-side crop render is running",
    };
  }
  if (image.preview_retouch_pending && image.preview_status === "queued") {
    return {
      state: "retouch-queued",
      text: "crop queued",
      title: "server-side crop render is queued",
    };
  }
  return {
    state: image.preview_status || "waiting",
    text: image.preview_status === "done" ? "ready" : image.preview_status || "waiting",
    title: image.preview_error || image.preview_status || "waiting for image render",
  };
}

function profileDisplayState(image, profile) {
  if (!profile) {
    return {
      state: "waiting",
      text: "waiting",
      title: "waiting for profile render",
    };
  }
  if (isLocalRetouchDraft(image)) {
    return {
      state: "retouch-draft",
      text: "retouch draft",
      title: "local draft preview; server render will queue after edits settle",
    };
  }
  if (profile.retouch_pending && profile.status === "processing") {
    return {
      state: "retouch-processing",
      text: "retouch rendering",
      title: "server-side retouch render is running",
    };
  }
  if (profile.retouch_pending && profile.status === "queued") {
    return {
      state: "retouch-queued",
      text: "retouch queued",
      title: "server-side retouch render is queued",
    };
  }
  return {
    state: profile.status || "waiting",
    text: profile.status || "waiting",
    title: profile.error || profile.status || "waiting",
  };
}

function publishProfileIndexes(image) {
  if (isDirectCompressedImage(image)) return [];
  if (Array.isArray(image.publish_profile_indexes)) return image.publish_profile_indexes;
  return (image.profiles || []).map((profile) => profile.profile_index);
}

function profilesAreImplicitOnly(image = null) {
  if (hasSoocProfile(image)) return false;
  const profiles = state.data?.profiles || [];
  return profiles.length === 1 && !String(profiles[0].selector || "").trim();
}

function visibleProfileCount(image = null) {
  if (profilesAreImplicitOnly(image)) return 0;
  return image ? image.profiles?.length || 0 : state.data?.profiles?.length || 0;
}

function enabledProfileIndexes(image) {
  return (image?.profiles || [])
    .filter((profile) => !isSoocProfile(profile) && profile.enabled !== false)
    .map((profile) => profile.profile_index);
}

function toggleEnabledProfile(image, profileIndex) {
  const enabled = new Set(enabledProfileIndexes(image));
  if (enabled.has(profileIndex)) {
    enabled.delete(profileIndex);
  } else {
    enabled.add(profileIndex);
  }
  return (image.profiles || []).map((profile) => profile.profile_index).filter((index) => enabled.has(index));
}

function normalizeBwFilter(value) {
  return BW_FILTERS.includes(value) ? value : "none";
}

function profileBwFilters(image) {
  const byIndex = new Map();
  const available = new Set((image?.profiles || []).map((profile) => profile.profile_index));
  for (const entry of image?.profile_bw_filters || []) {
    const profileIndex = Number(entry?.profile_index);
    const filter = normalizeBwFilter(entry?.filter);
    if (!Number.isInteger(profileIndex) || !available.has(profileIndex) || filter === "none") continue;
    byIndex.set(profileIndex, filter);
  }
  return (image?.profiles || [])
    .map((profile) => {
      const filter = byIndex.get(profile.profile_index);
      return filter ? { profile_index: profile.profile_index, filter } : null;
    })
    .filter(Boolean);
}

function nextProfileBwFilters(image, profileIndex, filter) {
  const byIndex = new Map(profileBwFilters(image).map((entry) => [entry.profile_index, entry.filter]));
  const normalized = normalizeBwFilter(filter);
  if (normalized === "none") {
    byIndex.delete(profileIndex);
  } else {
    byIndex.set(profileIndex, normalized);
  }
  return (image?.profiles || [])
    .map((profile) => {
      const filter = byIndex.get(profile.profile_index);
      return filter ? { profile_index: profile.profile_index, filter } : null;
    })
    .filter(Boolean);
}

async function setProfileBwFilter(image, profileIndex, filter) {
  const next = nextProfileBwFilters(image, profileIndex, filter);
  image.profile_bw_filters = next;
  const profile = (image.profiles || []).find((profile) => profile.profile_index === profileIndex);
  if (profile) {
    profile.bw_filter = normalizeBwFilter(filter);
    profile.retouch_pending = true;
    profile.status = "queued";
  }
  render();
  await saveReview({ profile_bw_filters: next });
}

function renderProfiles(image) {
  if (profilesAreImplicitOnly(image)) {
    preactRender(null, els.profiles);
    return;
  }
  preactRender(
    h(ProfileList, {
      image,
      onSelect: async (profile) => {
        const patch = { selected_profile_index: profile.profile_index };
        if (profile.enabled === false) {
          patch.enabled_profile_indexes = toggleEnabledProfile(image, profile.profile_index);
        }
        await saveReview(patch);
      },
      onToggleEnabled: async (profile) => {
        await saveReview({ enabled_profile_indexes: toggleEnabledProfile(image, profile.profile_index) });
      },
      onSolo: async (profile) => {
        await saveReview({
          selected_profile_index: profile.profile_index,
          enabled_profile_indexes: isSoocProfile(profile) ? [] : [profile.profile_index],
        });
      },
    }),
    els.profiles,
  );
}

const profileDoubleTap = {
  profileIndex: null,
  at: 0,
};

function ProfileList({ image, onSelect, onToggleEnabled, onSolo }) {
  if (!image) return null;
  const previewProfile = selectedProfile(image);
  const profiles = image.profiles || [];
  const canSolo = profiles.length > 1;
  return profiles.map((profile) => {
    const displayName = profileDisplayName(profile);
    const downloadTitle = profileDownloadTitle(profile, displayName);
    const cardUrl = profile.url || image.preview_url;
    const available = isSoocProfile(profile) || profile.enabled !== false;
    const display = profileDisplayState(image, profile);
    const isPortrait = isPortraitRenderProfile(profile);
    const sourceStatus = profile.url ? display.text : `${display.text} | preview`;
    const classes = [
      "profile-card",
      profile.profile_index === previewProfile?.profile_index ? "active" : "",
      profile.url ? "" : "pending",
      isPortrait ? "portrait" : "",
      display.state,
      available ? "availability-enabled" : "availability-disabled",
    ]
      .filter(Boolean)
      .join(" ");
    return h(
      "div",
      {
        key: profile.profile_index,
        class: "profile-entry",
      },
      h(
        "button",
        {
          type: "button",
          class: classes,
          onClick: () => onSelect(profile).catch((error) => console.error(error)),
          onDblClick: (event) => {
            if (!canSolo) return;
            event.preventDefault();
            onSolo(profile).catch((error) => console.error(error));
          },
          onPointerUp: (event) => {
            if (!canSolo || event.pointerType === "mouse") return;
            const now = Date.now();
            const sameProfile = profileDoubleTap.profileIndex === profile.profile_index;
            const isDoubleTap = sameProfile && now - profileDoubleTap.at < 450;
            profileDoubleTap.profileIndex = profile.profile_index;
            profileDoubleTap.at = now;
            if (!isDoubleTap) return;
            event.preventDefault();
            onSolo(profile).catch((error) => console.error(error));
          },
        },
        h("input", {
          type: "checkbox",
          class: "profile-availability",
          checked: available,
          disabled: isSoocProfile(profile),
          title: isSoocProfile(profile)
            ? "SOOC remains available"
            : available
              ? "Available for this picture"
              : "Disabled for this picture",
          "aria-label": `Enable ${displayName}`,
          onClick: (event) => event.stopPropagation(),
          onChange: (event) => {
            event.stopPropagation();
            onToggleEnabled(profile).catch((error) => console.error(error));
          },
        }),
        cardUrl
          ? h("img", {
              src: versionedUrl(cardUrl, profile.url ? profile.updated_at : image.preview_updated_at),
              alt: displayName,
              loading: profile.profile_index === previewProfile?.profile_index ? "eager" : "lazy",
              decoding: "async",
              fetchpriority: profile.profile_index === previewProfile?.profile_index ? "high" : "low",
              onLoad: (event) => {
                if (isPortraitRenderProfile(profile)) return;
                event.currentTarget
                  .closest(".profile-card")
                  ?.classList.toggle("portrait", event.currentTarget.naturalHeight > event.currentTarget.naturalWidth);
              },
            })
          : null,
        h("div", { class: "profile-name" }, displayName),
        h(
          "div",
          {
            class: "profile-status",
            title: display.title,
          },
          `${sourceStatus} | ${available ? "available" : "off"}`,
        ),
      ),
      profile.url
        ? h(
            "a",
            {
              class: "profile-download",
              href: versionedUrl(profile.url, profile.updated_at),
              download: profileDownloadName(image, profile),
              title: downloadTitle,
              "aria-label": downloadTitle,
              onClick: (event) => event.stopPropagation(),
            },
            "DL",
          )
        : null,
    );
  });
}

function profileDownloadName(image, profile) {
  const rawName = image.file_name || image.relative_path || "mini-film";
  const baseName = rawName.replace(/\.[^.]*$/, "");
  const profileName = profile.profile_stem || profile.selector || "profile";
  return `${safeDownloadPart(baseName)}--${safeDownloadPart(profileName)}.jpg`;
}

function profileDownloadTitle(profile, displayName) {
  const rawBytes = profile.file_size_bytes;
  const bytes = rawBytes === null || rawBytes === undefined || rawBytes === "" ? Number.NaN : Number(rawBytes);
  const size = Number.isFinite(bytes) && bytes >= 0 ? `${(bytes / 1_000_000).toFixed(1)} MB` : "";
  return size ? `Download rendered ${displayName} (${size})` : `Download rendered ${displayName}`;
}

function safeDownloadPart(value) {
  return String(value || "image")
    .trim()
    .replace(/[\\/:*?"<>|]+/g, "-")
    .split("")
    .filter((char) => char >= " ")
    .join("")
    .replace(/\s+/g, " ")
    .replace(/^-+|-+$/g, "")
    .slice(0, 120);
}

function isMobileReviewLayout() {
  return mobileReviewQuery.matches;
}

function setMobileDrawer(drawer) {
  const nextDrawer = isMobileReviewLayout() ? drawer : null;
  state.mobileDrawer = nextDrawer;
  els.app.classList.toggle("mobile-drawer-open", Boolean(nextDrawer));
  for (const name of ["profiles", "retouch", "metadata"]) {
    els.app.classList.toggle(`mobile-drawer-${name}`, nextDrawer === name);
  }
  els.mobileDrawerButtons.forEach((button) => {
    const active = nextDrawer === button.dataset.mobileDrawer;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  scheduleViewerSafeAreaUpdate();
}

function toggleMobileDrawer(drawer) {
  setMobileDrawer(state.mobileDrawer === drawer ? null : drawer);
}

function syncMobileReviewLayout() {
  if (!isMobileReviewLayout() && state.mobileDrawer) {
    setMobileDrawer(null);
    return;
  }
  setMobileDrawer(state.mobileDrawer);
}

function updateMobileActionLabels(image) {
  const profileCount = visibleProfileCount(image);
  const tagsCount = image?.tags?.length || 0;
  const hasNotes = Boolean(image?.notes);
  const directCompressed = isDirectCompressedImage(image);
  if (directCompressed && state.mobileDrawer === "retouch") setMobileDrawer(null);
  syncMobileSaveOriginal(image);
  const retouchActive = image
    ? directCompressed
      ? hasCropAdjustment(image)
      : !retouchIsDefault(image.retouch || defaultRetouch())
    : false;
  els.mobileDrawerButtons.forEach((button) => {
    const drawer = button.dataset.mobileDrawer;
    if (drawer === "profiles") {
      button.hidden = profilesAreImplicitOnly(image) || directCompressed;
      button.textContent = profileCount > 0 ? `Profiles ${profileCount}` : "Profiles";
      button.title = `${profileCount} profile ${profileCount === 1 ? "render" : "renders"}`;
    } else if (drawer === "retouch") {
      button.hidden = directCompressed;
      button.textContent = retouchActive ? "Retouch *" : "Retouch";
      button.title = retouchActive ? "Retouch adjustments are active" : "Retouch";
    } else if (drawer === "metadata") {
      button.textContent = tagsCount > 0 || hasNotes ? "Meta *" : "Meta";
      button.title = `${tagsCount} ${plural(tagsCount, "tag")}${hasNotes ? ", notes present" : ""}`;
    }
  });
}

function syncMobileSaveOriginal(image) {
  const button = els.mobileSaveOriginal;
  const compressed = isCompressedImage(image);
  button.hidden = !compressed;
  button.disabled = !compressed || state.originalShare.busyImageId === image.id;
  if (!compressed) {
    button.textContent = "Save Photo";
    button.removeAttribute("title");
    button.removeAttribute("aria-label");
    return;
  }

  if (state.originalShare.busyImageId === image.id) {
    button.textContent = "Preparing";
  } else if (state.originalShare.openImageId === image.id) {
    button.textContent = "Open Photo";
  } else if (state.originalShare.retryImageId === image.id) {
    button.textContent = "Save Again";
  } else {
    button.textContent = "Save Photo";
  }
  const action = state.originalShare.openImageId === image.id ? "Open" : "Save";
  button.title = `${action} original ${image.file_name || "photo"}`;
  button.setAttribute("aria-label", button.title);
}

function originalPhotoUrl(image) {
  return reviewUrl(`original/${image.id}`);
}

function openOriginalPhoto(image) {
  window.open(originalPhotoUrl(image), "_blank", "noopener");
}

function supportsOriginalFileShare() {
  return (
    typeof File === "function" && typeof navigator.share === "function" && typeof navigator.canShare === "function"
  );
}

async function originalFileForShare(image) {
  const share = state.originalShare;
  if (share.imageId === image.id && share.file) return share.file;
  if (share.imageId === image.id && share.promise) return share.promise;

  share.imageId = image.id;
  share.file = null;
  const promise = (async () => {
    const response = await fetch(originalPhotoUrl(image), { cache: "no-store" });
    if (!response.ok) throw new Error(`original ${response.status}`);
    const contentType = (response.headers.get("content-type") || "").split(";", 1)[0].trim().toLowerCase();
    if (!["image/jpeg", "image/heic", "image/heif"].includes(contentType)) {
      throw new Error(`unexpected original content type: ${contentType || "missing"}`);
    }
    const bytes = await response.blob();
    const fallbackName = contentType === "image/jpeg" ? "photo.jpg" : "photo.heic";
    return new File([bytes], image.file_name || fallbackName, { type: contentType });
  })();
  share.promise = promise;
  try {
    const file = await promise;
    if (share.imageId === image.id) share.file = file;
    return file;
  } finally {
    if (share.imageId === image.id) share.promise = null;
  }
}

async function saveOriginalPhoto() {
  const image = findImage(state.currentId);
  if (!isCompressedImage(image)) return;
  const share = state.originalShare;
  if (share.openImageId === image.id) {
    share.openImageId = null;
    syncMobileSaveOriginal(image);
    openOriginalPhoto(image);
    return;
  }
  if (!supportsOriginalFileShare()) {
    openOriginalPhoto(image);
    return;
  }

  share.busyImageId = image.id;
  share.retryImageId = null;
  syncMobileSaveOriginal(image);
  try {
    const file = await originalFileForShare(image);
    const shareData = { files: [file] };
    if (!navigator.canShare(shareData)) {
      share.openImageId = image.id;
      showGestureFeedback("open photo");
      return;
    }
    await navigator.share(shareData);
    share.openImageId = null;
  } catch (error) {
    if (error?.name === "AbortError") return;
    if (error?.name === "NotAllowedError" && share.imageId === image.id && share.file) {
      share.retryImageId = image.id;
      showGestureFeedback("photo ready");
      return;
    }
    console.error(error);
    share.openImageId = image.id;
    showGestureFeedback("open photo");
  } finally {
    if (share.busyImageId === image.id) share.busyImageId = null;
    syncMobileSaveOriginal(findImage(state.currentId));
  }
}

function preloadNearbyImages(image) {
  const urls = new Set();
  const compressedOnly = isCompressedOnlyReview();
  const candidates = compressedOnly ? nextImages(image.id, 3) : nearbyImages(image.id);
  const preloadFullMedia = compressedOnly && compressedViewportUsesFullMedia();

  for (const nearby of candidates) {
    const selected = selectedProfile(nearby);
    if (selected?.url) {
      urls.add(versionedUrl(selected.url, selected.updated_at));
    } else if (isDirectCompressedImage(nearby) && preloadFullMedia && nearby.full_url) {
      urls.add(versionedUrl(nearby.full_url, nearby.preview_updated_at || nearby.updated_at));
    } else if (nearby.preview_url) {
      urls.add(versionedUrl(nearby.preview_url, nearby.preview_updated_at));
    }
  }

  scheduleIdlePreloads(urls);
}

function isCompressedOnlyReview() {
  const images = state.data?.images || [];
  return images.length > 0 && images.every(isCompressedImage);
}

function nextImages(imageId, count) {
  const images = filteredImages();
  const index = images.findIndex((image) => image.id === imageId);
  if (index < 0) return [];
  return images.slice(index + 1, index + 1 + count);
}

function nearbyImages(imageId) {
  const images = filteredImages();
  const index = images.findIndex((image) => image.id === imageId);
  if (index < 0) return [];
  return [images[index - 1], images[index + 1]].filter(Boolean);
}

function versionedUrl(url, updatedAt) {
  return `${url}?v=${encodeURIComponent(updatedAt || "")}`;
}

function preloadImage(url) {
  if (!url || state.preloaded.has(url)) return;
  state.preloaded.add(url);
  const image = new Image();
  image.decoding = "async";
  image.loading = "eager";
  image.fetchPriority = "low";
  image.src = url;
  if (state.preloaded.size > 96) {
    state.preloaded = new Set(Array.from(state.preloaded).slice(-64));
  }
}

function scheduleIdlePreloads(urls) {
  if (urls.size === 0) return;
  const run = () => {
    for (const url of urls) preloadImage(url);
  };
  if ("requestIdleCallback" in window) {
    window.requestIdleCallback(run, { timeout: 1200 });
  } else {
    window.setTimeout(run, 350);
  }
}

async function toggleFullscreen() {
  if (document.fullscreenElement) {
    await document.exitFullscreen();
    return;
  }
  await els.app.requestFullscreen();
}

function toggleShortcuts(force) {
  const show = force ?? els.shortcutsOverlay.hidden;
  els.shortcutsOverlay.hidden = !show;
}

function toggleHistogram(force) {
  const show = force ?? !state.histogramOpen;
  state.histogramOpen = show;
  els.histogramOverlay.hidden = !show;
  if (show) {
    scheduleHistogramRender();
    return;
  }
  clearTimeout(state.histogramTimer);
  state.histogramTimer = null;
  state.histogramRequestId += 1;
}

function scheduleHistogramRender({ debounce = false } = {}) {
  if (!state.histogramOpen) return;
  clearTimeout(state.histogramTimer);
  const requestId = ++state.histogramRequestId;
  const delay = debounce ? HISTOGRAM_RETOUCH_DEBOUNCE_MS : 0;
  state.histogramTimer = setTimeout(() => {
    state.histogramTimer = null;
    renderHistogram(requestId);
  }, delay);
}

function renderHistogram(requestId) {
  if (!state.histogramOpen || requestId !== state.histogramRequestId) return;
  const image = els.image;
  const source = image.currentSrc || image.src;
  if (!source) {
    showHistogramEmpty("No image");
    return;
  }
  if (!image.complete || image.naturalWidth < 1 || image.naturalHeight < 1) {
    showHistogramEmpty("Loading");
    return;
  }

  const longest = Math.max(image.naturalWidth, image.naturalHeight);
  const scale = Math.min(1, HISTOGRAM_SAMPLE_LONG_EDGE / longest);
  const width = Math.max(1, Math.round(image.naturalWidth * scale));
  const height = Math.max(1, Math.round(image.naturalHeight * scale));
  const sampleCanvas = document.createElement("canvas");
  sampleCanvas.width = width;
  sampleCanvas.height = height;
  const sampleContext = sampleCanvas.getContext("2d", { willReadFrequently: true });
  if (!sampleContext) {
    showHistogramEmpty("Unavailable");
    return;
  }

  const imageFilter = window.getComputedStyle(image).filter;
  if ("filter" in sampleContext && imageFilter && imageFilter !== "none") {
    sampleContext.filter = imageFilter;
  }

  try {
    sampleContext.drawImage(image, 0, 0, width, height);
    drawHistogram(histogramBins(sampleContext.getImageData(0, 0, width, height).data));
  } catch (error) {
    console.error(error);
    showHistogramEmpty("Unavailable");
  }
}

function histogramBins(pixels) {
  const bins = {
    luma: new Uint32Array(256),
    red: new Uint32Array(256),
    green: new Uint32Array(256),
    blue: new Uint32Array(256),
  };
  for (let index = 0; index < pixels.length; index += 4) {
    const alpha = pixels[index + 3];
    if (alpha === 0) continue;
    const red = pixels[index];
    const green = pixels[index + 1];
    const blue = pixels[index + 2];
    const luma = clamp(Math.round(red * 0.2126 + green * 0.7152 + blue * 0.0722), 0, 255);
    bins.red[red] += 1;
    bins.green[green] += 1;
    bins.blue[blue] += 1;
    bins.luma[luma] += 1;
  }
  return bins;
}

function drawHistogram(bins) {
  els.histogramEmpty.hidden = true;
  const canvas = els.histogramCanvas;
  const context = resizeHistogramCanvas(canvas);
  if (!context) {
    showHistogramEmpty("Unavailable");
    return;
  }
  const { ctx, width, height } = context;
  ctx.clearRect(0, 0, width, height);
  drawHistogramGrid(ctx, width, height);
  drawHistogramFill(ctx, bins.luma, width, height);
  drawHistogramLine(ctx, bins.red, width, height, "rgba(255, 74, 74, 0.92)");
  drawHistogramLine(ctx, bins.green, width, height, "rgba(65, 210, 116, 0.92)");
  drawHistogramLine(ctx, bins.blue, width, height, "rgba(85, 154, 255, 0.92)");
}

function resizeHistogramCanvas(canvas) {
  const rect = canvas.getBoundingClientRect();
  const pixelRatio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round((rect.width || 512) * pixelRatio));
  const height = Math.max(1, Math.round((rect.height || 128) * pixelRatio));
  if (canvas.width !== width) canvas.width = width;
  if (canvas.height !== height) canvas.height = height;
  const ctx = canvas.getContext("2d");
  return ctx ? { ctx, width, height } : null;
}

function drawHistogramGrid(ctx, width, height) {
  ctx.save();
  ctx.strokeStyle = "rgba(255, 255, 255, 0.12)";
  ctx.lineWidth = Math.max(1, width / 512);
  ctx.beginPath();
  for (const x of [0.25, 0.5, 0.75]) {
    ctx.moveTo(Math.round(width * x), 0);
    ctx.lineTo(Math.round(width * x), height);
  }
  ctx.moveTo(0, Math.round(height * 0.5));
  ctx.lineTo(width, Math.round(height * 0.5));
  ctx.stroke();
  ctx.restore();
}

function drawHistogramFill(ctx, bins, width, height) {
  const max = histogramMax(bins);
  if (max <= 0) return;
  ctx.save();
  ctx.fillStyle = "rgba(255, 255, 255, 0.3)";
  ctx.strokeStyle = "rgba(255, 255, 255, 0.84)";
  ctx.lineWidth = Math.max(1, width / 512);
  ctx.beginPath();
  ctx.moveTo(0, height);
  for (let index = 0; index < bins.length; index++) {
    const x = (index / 255) * width;
    const y = height - (bins[index] / max) * (height - 2);
    ctx.lineTo(x, y);
  }
  ctx.lineTo(width, height);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  ctx.restore();
}

function drawHistogramLine(ctx, bins, width, height, color) {
  const max = histogramMax(bins);
  if (max <= 0) return;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(1.2, width / 380);
  ctx.beginPath();
  for (let index = 0; index < bins.length; index++) {
    const x = (index / 255) * width;
    const y = height - (bins[index] / max) * (height - 2);
    if (index === 0) {
      ctx.moveTo(x, y);
    } else {
      ctx.lineTo(x, y);
    }
  }
  ctx.stroke();
  ctx.restore();
}

function histogramMax(bins) {
  return bins.reduce((max, value) => Math.max(max, value), 0);
}

function showHistogramEmpty(message) {
  const canvas = els.histogramCanvas;
  const ctx = canvas.getContext("2d");
  if (ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
  els.histogramEmpty.textContent = message;
  els.histogramEmpty.hidden = false;
}

function togglePublishWizard(force) {
  const show = force ?? els.publishOverlay.hidden;
  if (show) {
    populatePublishWizard();
  }
  els.publishOverlay.hidden = !show;
}

function currentPanoramaProject() {
  if (state.panoramaProjectId === null) return null;
  return (state.data?.panorama?.projects || []).find((project) => project.id === state.panoramaProjectId) || null;
}

function renderPanoramaWizard() {
  if (!state.panoramaOpen) {
    els.panoramaOverlay.hidden = true;
    return;
  }
  els.panoramaOverlay.hidden = false;
  preactRender(h(PanoramaOverlay), els.panoramaOverlay);
}

function openPanoramaWizard() {
  if (!state.data?.capabilities?.panorama?.available) return;
  state.panoramaOpen = true;
  state.panoramaMessage = "";
  if (state.panoramaProjectId === null) initializeNewPanorama();
  renderPanoramaWizard();
}

function closePanoramaWizard() {
  state.panoramaOpen = false;
  els.panoramaOverlay.hidden = true;
}

function normalizeDiffusionSettings(settings) {
  const method = DIFFUSION_METHODS.some((candidate) => candidate.id === settings?.method)
    ? settings.method
    : DIFFUSION_METHODS[0].id;
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

function normalizeDiffusionAmount(value, min, max, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.round(clamp(number, min, max)) : fallback;
}

function diffusionSettingsSignature(settings) {
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

function diffusionPresetSettings(preset, method) {
  return {
    softness: preset.softness,
    highlight_glow: preset.highlight_glow,
    softness_radius_percent: preset.softness_radius_percent,
    glow_radius_percent: preset.glow_radius_percent,
    intensity_percent: preset.intensity_percent,
    highlight_reach: method === "edge-aware-glow" ? preset.highlight_reach : 50,
  };
}

function diffusionPresetIsActive(preset, settings) {
  if (preset.id === "off") return settings.softness === 0 && settings.highlight_glow === 0;
  const expected = diffusionPresetSettings(preset, settings.method);
  return Object.entries(expected).every(([key, value]) => settings[key] === value);
}

function diffusionProfile(image, profileIndex) {
  return (image?.profiles || []).find((profile) => profile.profile_index === profileIndex) || null;
}

function currentDiffusionContext() {
  const image = findImage(state.currentId);
  const profile = selectedProfile(image);
  if (!image || !profile || isDirectCompressedImage(image) || isSoocProfile(profile)) return null;
  const profileIndex = Number(profile.profile_index);
  return Number.isFinite(profileIndex) ? { image, profile, profileIndex } : null;
}

function effectiveDiffusion(profile) {
  const nested = profile?.diffusion;
  return {
    settings: nested?.settings || profile?.diffusion_settings || null,
    source: nested?.source ?? profile?.diffusion_source ?? null,
  };
}

function updateDiffusionButton(image) {
  const profile = selectedProfile(image);
  const settings = normalizeDiffusionSettings(effectiveDiffusion(profile).settings);
  const active = Boolean(
    image &&
    profile &&
    !isDirectCompressedImage(image) &&
    !isSoocProfile(profile) &&
    (settings.softness > 0 || settings.highlight_glow > 0),
  );
  els.diffusion.classList.toggle("active", active);
  els.diffusion.title = active ? "Diffusion applied" : "Open diffusion tools";
  els.diffusion.setAttribute("aria-label", active ? "Open diffusion tools, diffusion applied" : "Open diffusion tools");
}

function diffusionSourceLabel(source) {
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

function diffusionBeforeSource(job) {
  if (state.diffusionBefore?.url && job?.status !== "done") return state.diffusionBefore;
  const url = job?.before_url || job?.source_url;
  return url
    ? { url, updatedAt: job?.before_updated_at || job?.updated_at }
    : state.diffusionBefore || { url: null, updatedAt: null };
}

function diffusionAfterSource(job) {
  return {
    url: job?.after_url || job?.preview_url || job?.result_url || null,
    updatedAt: job?.after_updated_at || job?.updated_at,
  };
}

function diffusionPreviewContext(job) {
  const remembered = state.diffusionPreviewContext;
  const jobWidth = Number(job?.preview_width);
  const jobHeight = Number(job?.preview_height);
  const width = Number.isFinite(jobWidth) && jobWidth > 0 ? Math.round(jobWidth) : remembered?.width;
  const height = Number.isFinite(jobHeight) && jobHeight > 0 ? Math.round(jobHeight) : remembered?.height;
  if (!width || !height) return remembered;

  const sameDimensions = remembered?.width === width && remembered?.height === height;
  let areas = sameDimensions ? remembered.areas : [];
  if (Array.isArray(job?.detail_areas) && job.detail_areas.length > 0) {
    const normalizedAreas = job.detail_areas
      .map((area) => normalizeDiffusionDetailArea(area, width, height))
      .filter(Boolean);
    if (normalizedAreas.length > 0) areas = normalizedAreas;
  }

  const focusSource = ["camera-focus", "center-fallback"].includes(job?.focus_source)
    ? job.focus_source
    : sameDimensions
      ? remembered.focusSource
      : null;
  return { width, height, focusSource, areas };
}

function normalizeDiffusionDetailArea(area, previewWidth, previewHeight) {
  if (!DIFFUSION_DETAIL_AREAS.some((definition) => definition.kind === area?.kind)) return null;
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

function rememberDiffusionPreviewContext(job) {
  const context = diffusionPreviewContext(job);
  if (context) state.diffusionPreviewContext = context;
}

function diffusionDetailFrameStyle(area) {
  return area ? { aspectRatio: `${area.width} / ${area.height}` } : { aspectRatio: "1 / 1" };
}

function diffusionDetailMediaStyle(area, previewContext) {
  return {
    width: `${((previewContext.width / area.width) * 100).toFixed(6)}%`,
    height: `${((previewContext.height / area.height) * 100).toFixed(6)}%`,
    left: `${((-area.x / area.width) * 100).toFixed(6)}%`,
    top: `${((-area.y / area.height) * 100).toFixed(6)}%`,
  };
}

function diffusionMediaStyle(job, image, profile) {
  const previewContext = diffusionPreviewContext(job);
  const width = Number(previewContext?.width || job?.source_width || profile?.width || image?.source_width);
  const height = Number(previewContext?.height || job?.source_height || profile?.height || image?.source_height);
  return Number.isFinite(width) && width > 0 && Number.isFinite(height) && height > 0
    ? { aspectRatio: `${width} / ${height}` }
    : undefined;
}

function diffusionStatusText(job) {
  if (state.diffusionSaving) return state.diffusionMessage || "Saving diffusion settings";
  if (!job) return state.diffusionLoading ? "Preparing preview" : "Preview unavailable";
  if (job.status === "done") return diffusionAfterSource(job).url ? "Preview ready" : "Preview output unavailable";
  if (job.status === "failed") return job.error || "Preview failed";
  if (job.status === "processing") return "Rendering preview";
  if (job.status === "queued") return "Preview queued";
  return state.diffusionLoading ? "Preparing preview" : capitalize(job.status || "preview");
}

function diffusionJobIsTerminal(job) {
  return ["done", "failed", "cancelled"].includes(job?.status);
}

function renderDiffusion() {
  if (!state.diffusionOpen) {
    els.diffusionOverlay.hidden = true;
    return;
  }
  els.diffusionOverlay.hidden = false;
  preactRender(h(DiffusionOverlay), els.diffusionOverlay);
}

function openDiffusion() {
  const context = currentDiffusionContext();
  if (!context) return;
  const effective = effectiveDiffusion(context.profile);
  state.diffusionOpen = true;
  state.diffusionLoading = false;
  state.diffusionSaving = false;
  state.diffusionError = "";
  state.diffusionErrorKind = null;
  state.diffusionMessage = "";
  state.diffusionJob = null;
  state.diffusionBefore = null;
  state.diffusionPreviewContext = null;
  state.diffusionImageId = context.image.id;
  state.diffusionProfileIndex = context.profileIndex;
  state.diffusionSettings = normalizeDiffusionSettings(effective.settings);
  state.diffusionSource = effective.source;
  state.diffusionRequestedSignature = "";
  renderDiffusion();
  requestDiffusionPreview();
}

function closeDiffusion() {
  if (state.diffusionSaving) return;
  state.diffusionOpen = false;
  cancelDiffusionPreviewRequests();
  state.diffusionJob = null;
  state.diffusionBefore = null;
  state.diffusionPreviewContext = null;
  state.diffusionImageId = null;
  state.diffusionProfileIndex = null;
  state.diffusionSettings = null;
  state.diffusionSource = null;
  state.diffusionRequestedSignature = "";
  state.diffusionErrorKind = null;
  els.diffusionOverlay.hidden = true;
  preactRender(null, els.diffusionOverlay);
}

function setDiffusionSettings(patch) {
  if (!state.diffusionOpen || state.diffusionSaving) return;
  const next = normalizeDiffusionSettings({ ...state.diffusionSettings, ...patch });
  if (diffusionSettingsSignature(next) === diffusionSettingsSignature(state.diffusionSettings)) return;
  state.diffusionSettings = next;
  cancelDiffusionPreviewRequests();
  state.diffusionRequestedSignature = "";
  state.diffusionJob = null;
  state.diffusionLoading = true;
  state.diffusionError = "";
  state.diffusionErrorKind = null;
  state.diffusionMessage = "";
  renderDiffusion();
  scheduleDiffusionPreview();
}

function cancelDiffusionPreviewRequests() {
  state.diffusionPreviewRequestId += 1;
  clearTimeout(state.diffusionPollTimer);
  clearTimeout(state.diffusionPreviewTimer);
  state.diffusionPollTimer = null;
  state.diffusionPreviewTimer = null;
  state.diffusionController?.abort();
  state.diffusionController = null;
  state.diffusionLoading = false;
}

function scheduleDiffusionPreview() {
  clearTimeout(state.diffusionPreviewTimer);
  state.diffusionPreviewTimer = setTimeout(requestDiffusionPreview, DIFFUSION_PREVIEW_DEBOUNCE_MS);
}

async function diffusionRequest(path, method, body, signal) {
  const response = await fetch(reviewUrl(path), {
    method,
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
  });
  let data = null;
  try {
    data = await response.json();
  } catch {
    // Settings deletion may return an empty response.
  }
  if (!response.ok) throw new Error(data?.error || `diffusion ${response.status}`);
  return data;
}

async function requestDiffusionPreview() {
  clearTimeout(state.diffusionPreviewTimer);
  state.diffusionPreviewTimer = null;
  if (!state.diffusionOpen || !state.diffusionSettings) return;
  const settings = normalizeDiffusionSettings(state.diffusionSettings);
  const signature = `${state.diffusionImageId}:${state.diffusionProfileIndex}:${diffusionSettingsSignature(settings)}`;
  if (
    signature === state.diffusionRequestedSignature &&
    (state.diffusionLoading || state.diffusionJob?.status === "done")
  )
    return;

  state.diffusionRequestedSignature = signature;
  const requestId = state.diffusionPreviewRequestId + 1;
  state.diffusionPreviewRequestId = requestId;
  clearTimeout(state.diffusionPollTimer);
  state.diffusionPollTimer = null;
  state.diffusionController?.abort();
  const controller = new AbortController();
  state.diffusionController = controller;
  state.diffusionLoading = true;
  state.diffusionError = "";
  state.diffusionErrorKind = null;
  state.diffusionMessage = "";
  state.diffusionJob = null;
  renderDiffusion();

  try {
    const job = await diffusionRequest(
      "api/diffusion/jobs",
      "POST",
      {
        image_id: state.diffusionImageId,
        profile_index: state.diffusionProfileIndex,
        settings,
      },
      controller.signal,
    );
    if (!state.diffusionOpen || state.diffusionPreviewRequestId !== requestId) return;
    if (!job) throw new Error("preview job returned no data");
    state.diffusionJob = job;
    rememberDiffusionPreviewContext(job);
    if (job.before_url || job.source_url) state.diffusionBefore = diffusionBeforeSource(job);
    if (job.settings) state.diffusionSettings = normalizeDiffusionSettings(job.settings);
    state.diffusionLoading = !diffusionJobIsTerminal(job);
    state.diffusionError = job.status === "failed" ? job.error || "Preview failed" : "";
    state.diffusionErrorKind = state.diffusionError ? "preview" : null;
    renderDiffusion();
    scheduleDiffusionPoll(requestId);
  } catch (error) {
    if (error.name === "AbortError" || !state.diffusionOpen || state.diffusionPreviewRequestId !== requestId) return;
    state.diffusionLoading = false;
    state.diffusionError = error.message;
    state.diffusionErrorKind = "preview";
    state.diffusionRequestedSignature = "";
    renderDiffusion();
  } finally {
    if (state.diffusionController === controller) state.diffusionController = null;
  }
}

function scheduleDiffusionPoll(requestId) {
  clearTimeout(state.diffusionPollTimer);
  state.diffusionPollTimer = null;
  const job = state.diffusionJob;
  if (!state.diffusionOpen || state.diffusionPreviewRequestId !== requestId || diffusionJobIsTerminal(job)) return;
  if (!job?.id) {
    state.diffusionLoading = false;
    state.diffusionError = "Preview job did not return an id";
    state.diffusionErrorKind = "preview";
    state.diffusionRequestedSignature = "";
    renderDiffusion();
    return;
  }
  state.diffusionPollTimer = setTimeout(() => pollDiffusionJob(requestId, job.id), DIFFUSION_POLL_MS);
}

async function pollDiffusionJob(requestId, jobId) {
  state.diffusionPollTimer = null;
  if (!state.diffusionOpen || state.diffusionPreviewRequestId !== requestId) return;
  const controller = new AbortController();
  state.diffusionController?.abort();
  state.diffusionController = controller;
  try {
    const job = await diffusionRequest(`api/diffusion/jobs/${jobId}`, "GET", undefined, controller.signal);
    if (!state.diffusionOpen || state.diffusionPreviewRequestId !== requestId || state.diffusionJob?.id !== jobId)
      return;
    state.diffusionJob = job;
    rememberDiffusionPreviewContext(job);
    if (job?.before_url || job?.source_url) state.diffusionBefore = diffusionBeforeSource(job);
    if (job?.settings) state.diffusionSettings = normalizeDiffusionSettings(job.settings);
    state.diffusionLoading = !diffusionJobIsTerminal(job);
    state.diffusionError = job?.status === "failed" ? job.error || "Preview failed" : "";
    state.diffusionErrorKind = state.diffusionError ? "preview" : null;
    renderDiffusion();
  } catch (error) {
    if (error.name === "AbortError" || !state.diffusionOpen || state.diffusionPreviewRequestId !== requestId) return;
    state.diffusionError = error.message;
    state.diffusionErrorKind = "preview";
    renderDiffusion();
  } finally {
    if (state.diffusionController === controller) state.diffusionController = null;
  }
  scheduleDiffusionPoll(requestId);
}

async function applyDiffusion(scope) {
  if (!state.diffusionOpen || state.diffusionSaving || !state.diffusionSettings) return;
  cancelDiffusionPreviewRequests();
  state.diffusionSaving = true;
  state.diffusionError = "";
  state.diffusionErrorKind = null;
  state.diffusionMessage =
    scope === "all" ? "Applying to all pictures for this profile" : "Applying to current picture";
  renderDiffusion();
  try {
    const update = await diffusionRequest("api/diffusion/settings", "POST", {
      image_id: state.diffusionImageId,
      profile_index: state.diffusionProfileIndex,
      scope,
      settings: normalizeDiffusionSettings(state.diffusionSettings),
    });
    state.diffusionSaving = false;
    closeDiffusion();
    if (update) applyStateMessage(update);
  } catch (error) {
    state.diffusionSaving = false;
    state.diffusionError = `Could not apply diffusion: ${error.message}`;
    state.diffusionErrorKind = "save";
    state.diffusionMessage = "";
    renderDiffusion();
  }
}

async function resetDiffusion(scope) {
  if (!state.diffusionOpen || state.diffusionSaving) return;
  cancelDiffusionPreviewRequests();
  state.diffusionSaving = true;
  state.diffusionError = "";
  state.diffusionErrorKind = null;
  state.diffusionMessage = scope === "all" ? "Resetting this profile for all pictures" : "Resetting current picture";
  renderDiffusion();
  try {
    const update = await diffusionRequest("api/diffusion/settings", "DELETE", {
      image_id: state.diffusionImageId,
      profile_index: state.diffusionProfileIndex,
      scope,
    });
    state.diffusionSaving = false;
    closeDiffusion();
    if (update) applyStateMessage(update);
  } catch (error) {
    state.diffusionSaving = false;
    state.diffusionError = `Could not reset diffusion: ${error.message}`;
    state.diffusionErrorKind = "save";
    state.diffusionMessage = "";
    renderDiffusion();
  }
}

function renderSampler() {
  if (!state.samplerOpen) {
    els.samplerOverlay.hidden = true;
    return;
  }
  els.samplerOverlay.hidden = false;
  preactRender(h(SamplerOverlay), els.samplerOverlay);
  requestAnimationFrame(refreshSamplerPriorityObserver);
}

async function openSampler() {
  const image = findImage(state.currentId);
  if (!image || !state.data?.capabilities?.sampler) return;
  state.samplerOpen = true;
  state.samplerLoading = true;
  state.samplerError = "";
  state.samplerJob = null;
  state.samplerExpandedSections.clear();
  state.samplerKnownEnabledKeys.clear();
  state.samplerVisibleKeys.clear();
  state.samplerSelectedKey = null;
  state.samplerPrioritySignature = "";
  renderSampler();
  try {
    const job = await samplerRequest("api/sampler/jobs", "POST", { image_id: image.id });
    if (!state.samplerOpen) return;
    state.samplerJob = job;
    state.samplerSelectedKey =
      job.entries?.find((entry) => entry.selected)?.key ||
      job.entries?.find((entry) => entry.current_enabled)?.key ||
      null;
    syncSamplerAutoExpandedSections(job);
    state.samplerLoading = false;
    renderSampler();
    scheduleSamplerPoll();
  } catch (error) {
    if (!state.samplerOpen) return;
    state.samplerLoading = false;
    state.samplerError = error.message;
    renderSampler();
  }
}

function closeSampler() {
  state.samplerOpen = false;
  state.samplerLoading = false;
  clearTimeout(state.samplerPollTimer);
  clearTimeout(state.samplerPriorityTimer);
  state.samplerPollTimer = null;
  state.samplerPriorityTimer = null;
  state.samplerObserver?.disconnect();
  state.samplerObserver = null;
  state.samplerPriorityController?.abort();
  state.samplerPriorityController = null;
  state.samplerVisibleKeys.clear();
  els.samplerOverlay.hidden = true;
  preactRender(null, els.samplerOverlay);
}

async function samplerRequest(path, method, body) {
  const response = await fetch(reviewUrl(path), {
    method,
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data;
  try {
    data = await response.json();
  } catch {
    data = null;
  }
  if (!response.ok) throw new Error(data?.error || `sampler ${response.status}`);
  return data;
}

function scheduleSamplerPoll() {
  clearTimeout(state.samplerPollTimer);
  state.samplerPollTimer = null;
  if (!state.samplerOpen || !state.samplerJob || ["done", "failed"].includes(state.samplerJob.status)) return;
  state.samplerPollTimer = setTimeout(pollSamplerJob, SAMPLER_POLL_MS);
}

async function pollSamplerJob() {
  const jobId = state.samplerJob?.id;
  if (!state.samplerOpen || !jobId) return;
  try {
    const job = await samplerRequest(`api/sampler/jobs/${jobId}`, "GET");
    if (!state.samplerOpen || state.samplerJob?.id !== jobId) return;
    state.samplerJob = job;
    syncSamplerAutoExpandedSections(job);
    state.samplerError = "";
    renderSampler();
  } catch (error) {
    if (state.samplerOpen) {
      state.samplerError = error.message;
      renderSampler();
    }
  }
  scheduleSamplerPoll();
}

function toggleSamplerSection(key, expanded) {
  const changed = expanded ? !state.samplerExpandedSections.has(key) : state.samplerExpandedSections.has(key);
  if (!changed) return;
  if (expanded) {
    state.samplerExpandedSections.add(key);
  } else {
    state.samplerExpandedSections.delete(key);
  }
  requestAnimationFrame(refreshSamplerPriorityObserver);
}

function syncSamplerAutoExpandedSections(job) {
  const enabledKeys = new Set(
    (job?.entries || []).filter((entry) => entry.current_enabled || entry.selected).map((entry) => entry.key),
  );
  const newlyEnabled = Array.from(enabledKeys).filter((key) => !state.samplerKnownEnabledKeys.has(key));
  if (newlyEnabled.length > 0) {
    const hierarchy = buildSamplerHierarchy(job.entries || []);
    for (const entryKey of newlyEnabled) {
      const section = hierarchy.entrySections.get(entryKey);
      if (!section) continue;
      for (const ancestorKey of section.ancestorKeys) state.samplerExpandedSections.add(ancestorKey);
      state.samplerExpandedSections.add(section.key);
    }
  }
  state.samplerKnownEnabledKeys = enabledKeys;
}

function selectSamplerEntry(key) {
  state.samplerSelectedKey = key;
  renderSampler();
}

async function updateSamplerSelection(entry, scope, enabled) {
  const jobId = state.samplerJob?.id;
  if (!jobId || entry.status !== "done") return;
  const pendingKey = `${entry.key}:${scope}`;
  state.samplerPendingSelections.add(pendingKey);
  state.samplerError = "";
  renderSampler();
  try {
    const job = await samplerRequest(`api/sampler/jobs/${jobId}/profiles/${entry.key}`, "POST", {
      scope,
      enabled,
    });
    if (state.samplerOpen && state.samplerJob?.id === jobId) {
      state.samplerJob = job;
      if (enabled) state.samplerSelectedKey = entry.key;
      syncSamplerAutoExpandedSections(job);
    }
  } catch (error) {
    if (state.samplerOpen) state.samplerError = error.message;
  } finally {
    state.samplerPendingSelections.delete(pendingKey);
    if (state.samplerOpen) renderSampler();
  }
}

function refreshSamplerPriorityObserver() {
  state.samplerObserver?.disconnect();
  state.samplerObserver = null;
  if (!state.samplerOpen || !state.samplerJob) return;
  const root = els.samplerOverlay.querySelector(".sampler-sections");
  if (!root || !("IntersectionObserver" in window)) {
    scheduleSamplerPriorityUpdate();
    return;
  }
  const tiles = Array.from(root.querySelectorAll("[data-sampler-key]"));
  const availableKeys = new Set(tiles.map((tile) => tile.dataset.samplerKey));
  state.samplerVisibleKeys = new Set(Array.from(state.samplerVisibleKeys).filter((key) => availableKeys.has(key)));
  state.samplerObserver = new IntersectionObserver(
    (entries) => {
      let changed = false;
      for (const observed of entries) {
        const key = observed.target.dataset.samplerKey;
        if (!key) continue;
        if (observed.isIntersecting && !state.samplerVisibleKeys.has(key)) {
          state.samplerVisibleKeys.add(key);
          changed = true;
        } else if (!observed.isIntersecting && state.samplerVisibleKeys.delete(key)) {
          changed = true;
        }
      }
      if (changed) scheduleSamplerPriorityUpdate();
    },
    { root, rootMargin: "80px 0px", threshold: 0.01 },
  );
  tiles.forEach((tile) => state.samplerObserver.observe(tile));
  scheduleSamplerPriorityUpdate();
}

function scheduleSamplerPriorityUpdate() {
  clearTimeout(state.samplerPriorityTimer);
  state.samplerPriorityTimer = setTimeout(sendSamplerPriority, SAMPLER_PRIORITY_DEBOUNCE_MS);
}

function expandedSamplerKeys() {
  const keys = new Set();
  const hierarchy = buildSamplerHierarchy(state.samplerJob?.entries || []);
  const visit = (section, ancestorsExpanded) => {
    const expanded = ancestorsExpanded && state.samplerExpandedSections.has(section.key);
    if (expanded) {
      for (const entry of section.entries) keys.add(entry.key);
    }
    section.children.forEach((child) => visit(child, expanded));
  };
  hierarchy.sections.forEach((section) => visit(section, true));
  return Array.from(keys);
}

async function sendSamplerPriority() {
  state.samplerPriorityTimer = null;
  const jobId = state.samplerJob?.id;
  if (!state.samplerOpen || !jobId) return;
  const visibleKeys = Array.from(state.samplerVisibleKeys).sort();
  const expandedKeys = expandedSamplerKeys().sort();
  const signature = `${jobId}|${visibleKeys.join(",")}|${expandedKeys.join(",")}`;
  if (signature === state.samplerPrioritySignature) return;
  state.samplerPrioritySignature = signature;
  state.samplerPriorityController?.abort();
  const controller = new AbortController();
  state.samplerPriorityController = controller;
  try {
    await fetch(reviewUrl(`api/sampler/jobs/${jobId}/priority`), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ visible_keys: visibleKeys, expanded_keys: expandedKeys }),
      signal: controller.signal,
    });
  } catch (error) {
    if (error.name !== "AbortError") console.error(error);
  } finally {
    if (state.samplerPriorityController === controller) state.samplerPriorityController = null;
  }
}

function initializeNewPanorama() {
  const images = state.data?.images || [];
  const currentIndex = Math.max(
    0,
    images.findIndex((image) => image.id === state.currentId),
  );
  state.panoramaProjectId = null;
  state.panoramaImageIds = images.slice(currentIndex, currentIndex + 3).map((image) => image.id);
  if (state.panoramaImageIds.length < 2) {
    state.panoramaImageIds = images.slice(Math.max(0, images.length - 3)).map((image) => image.id);
  }
  const current = images[currentIndex];
  const stem = current?.file_name?.replace(/\.[^.]+$/, "") || "Panorama";
  state.panoramaName = `${stem} panorama`;
  state.panoramaMatching = "automatic";
  state.panoramaProjection = "cylindrical";
  state.panoramaMessage = "";
}

function selectPanoramaProject(value) {
  if (value === "new") {
    initializeNewPanorama();
    renderPanoramaWizard();
    return;
  }
  const projectId = Number(value);
  const project = (state.data?.panorama?.projects || []).find((candidate) => candidate.id === projectId);
  if (!project) return;
  state.panoramaProjectId = project.id;
  state.panoramaImageIds = [...(project.image_ids || [])];
  state.panoramaName = project.name || "Panorama";
  state.panoramaMatching = project.matching_mode || "automatic";
  state.panoramaProjection = project.selected_projection || "cylindrical";
  state.panoramaMessage = "";
  renderPanoramaWizard();
}

function togglePanoramaSource(imageId) {
  const index = state.panoramaImageIds.indexOf(imageId);
  if (index >= 0) {
    state.panoramaImageIds.splice(index, 1);
  } else {
    state.panoramaImageIds.push(imageId);
  }
  state.panoramaMessage = "";
  renderPanoramaWizard();
}

function movePanoramaSource(imageId, direction) {
  const index = state.panoramaImageIds.indexOf(imageId);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= state.panoramaImageIds.length) return;
  [state.panoramaImageIds[index], state.panoramaImageIds[target]] = [
    state.panoramaImageIds[target],
    state.panoramaImageIds[index],
  ];
  renderPanoramaWizard();
}

function panoramaStatusText(project) {
  if (!project) return "Select at least two sources";
  if (project.status === "ready") return "Previews ready";
  if (project.status === "complete") return project.output_file_name || "Panorama complete";
  if (project.status === "failed") return project.error || "Panorama failed";
  if (project.status === "interrupted") return project.error || "Panorama interrupted";
  if (project.status === "draft") return "Draft";
  const stage = String(project.progress_stage || project.status).replaceAll("-", " ");
  return capitalize(stage);
}

async function panoramaRequest(path, method, body) {
  const response = await fetch(reviewUrl(path), {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || `panorama ${response.status}`);
  applyStateMessage(data);
  return data;
}

async function generatePanoramaPreviews() {
  state.panoramaMessage = "Starting previews";
  renderPanoramaWizard();
  try {
    if (state.panoramaProjectId === null) {
      const existing = new Set((state.data?.panorama?.projects || []).map((project) => project.id));
      await panoramaRequest("api/panoramas", "POST", {
        image_ids: state.panoramaImageIds,
        name: state.panoramaName,
        matching_mode: state.panoramaMatching,
      });
      const created = (state.data?.panorama?.projects || [])
        .filter((project) => !existing.has(project.id))
        .sort((left, right) => right.id - left.id)[0];
      if (!created) throw new Error("created panorama project was not returned");
      state.panoramaProjectId = created.id;
    } else {
      await panoramaRequest(`api/panoramas/${state.panoramaProjectId}`, "PATCH", {
        image_ids: state.panoramaImageIds,
        name: state.panoramaName,
        matching_mode: state.panoramaMatching,
      });
    }
    await panoramaRequest(`api/panoramas/${state.panoramaProjectId}/previews`, "POST", {
      image_ids: state.panoramaImageIds,
      matching_mode: state.panoramaMatching,
    });
    state.panoramaMessage = "";
  } catch (error) {
    state.panoramaMessage = `Preview failed: ${error.message}`;
    renderPanoramaWizard();
  }
}

async function renderPanoramaFinal() {
  if (state.panoramaProjectId === null) return;
  state.panoramaMessage = "Starting full render";
  renderPanoramaWizard();
  try {
    await panoramaRequest(`api/panoramas/${state.panoramaProjectId}/render`, "POST", {
      name: state.panoramaName,
      projection: state.panoramaProjection,
    });
    state.panoramaMessage = "";
  } catch (error) {
    state.panoramaMessage = `Render failed: ${error.message}`;
    renderPanoramaWizard();
  }
}

function publishDefaults() {
  return state.data?.publish_defaults || {};
}

function populatePublishWizard() {
  const defaults = publishDefaults();
  els.publishAlbum.value = defaults.album || "published";
  els.publishMinRating.value = String(minRating());
  els.publishTags.value = "";
  document.querySelectorAll("[name='publish-label']").forEach((input) => {
    input.checked = false;
  });
  els.publishOutputFormat.value = defaults.output_format || "jpg";
  els.publishGrainEngine.value = defaults.grain_engine || "legacy";
  els.publishNormalizeGrain.checked = defaults.normalize_grain_mpix !== null;
  els.publishNormalizeGrainMpix.value = String(defaults.normalize_grain_mpix ?? 12);
  els.publishJpgQuality.value = String(defaults.jpg_quality || 95);
  els.publishJpegSubsampling.value = defaults.jpeg_subsampling || "s444";
  els.publishProgressive.checked = Boolean(defaults.progressive_jpeg);
  els.publishStripMetadata.checked = Boolean(defaults.strip_metadata);
  els.publishGallery.value = defaults.gallery || "none";
  els.publishGalleryColumns.value = String(defaults.gallery_columns || 4);
  els.publishGalleryThumbnailLongEdge.value = String(defaults.gallery_thumbnail_long_edge || 1024);

  els.publishResize.value = defaults.resize || "";
  els.publishLongEdge.value = defaults.long_edge ? String(defaults.long_edge) : "";
  els.publishMaxWidth.value = defaults.max_width ? String(defaults.max_width) : "";
  els.publishMaxHeight.value = defaults.max_height ? String(defaults.max_height) : "";
  if (defaults.resize) {
    els.publishSizeMode.value = "geometry";
  } else if (defaults.long_edge) {
    els.publishSizeMode.value = "long-edge";
  } else if (defaults.max_width || defaults.max_height) {
    els.publishSizeMode.value = "bounds";
  } else {
    els.publishSizeMode.value = "original";
  }
  syncPublishSizeFields();
  syncPublishNormalizeGrainField();
  updatePublishModeText();
}

function syncPublishNormalizeGrainField() {
  els.publishNormalizeGrainMpix.disabled = !els.publishNormalizeGrain.checked;
}

function syncPublishSizeFields() {
  const mode = els.publishSizeMode.value;
  els.publishLongEdge.hidden = mode !== "long-edge";
  els.publishMaxWidth.hidden = mode !== "bounds";
  els.publishMaxHeight.hidden = mode !== "bounds";
  els.publishResize.hidden = mode !== "geometry";
}

function publishFormBody() {
  const sizeMode = els.publishSizeMode.value;
  const body = {
    album: els.publishAlbum.value.trim() || "published",
    min_rating: Number(els.publishMinRating.value || 0),
    labels: Array.from(document.querySelectorAll("[name='publish-label']:checked")).map((input) => input.value),
    tags: splitPublishTags(els.publishTags.value),
    output_format: els.publishOutputFormat.value,
    grain_engine: els.publishGrainEngine.value,
    normalize_grain: els.publishNormalizeGrain.checked,
    normalize_grain_mpix: numberOrNull(els.publishNormalizeGrainMpix.value),
    gallery: els.publishGallery.value,
    size_mode: sizeMode,
    jpg_quality: Number(els.publishJpgQuality.value || 95),
    jpeg_subsampling: els.publishJpegSubsampling.value,
    strip_metadata: els.publishStripMetadata.checked,
    progressive_jpeg: els.publishProgressive.checked,
    gallery_columns: Number(els.publishGalleryColumns.value || 4),
    gallery_thumbnail_long_edge: Number(els.publishGalleryThumbnailLongEdge.value || 1024),
  };
  if (sizeMode === "long-edge") body.long_edge = numberOrNull(els.publishLongEdge.value);
  if (sizeMode === "bounds") {
    body.max_width = numberOrNull(els.publishMaxWidth.value);
    body.max_height = numberOrNull(els.publishMaxHeight.value);
  }
  if (sizeMode === "geometry") body.resize = els.publishResize.value.trim();
  return body;
}

function publishSelectionStats(body = publishFormBody()) {
  const labels = new Set((body.labels || []).filter(Boolean));
  const tags = new Set((body.tags || []).map((tag) => tag.toLowerCase()));
  let pictures = 0;
  let outputs = 0;
  for (const image of state.data?.images || []) {
    if (!imagePassesPublishFilters(image, body.min_rating, labels, tags)) continue;
    pictures += 1;
    outputs += isDirectCompressedImage(image) ? 1 : publishProfileIndexes(image).length;
  }
  return { pictures, outputs };
}

function imagePassesPublishFilters(image, minRatingValue, labels, tags) {
  if (Number(image.rating || 0) < Number(minRatingValue || 0)) return false;
  if (labels.size > 0 && imageLabels(image).every((label) => !labels.has(label))) return false;
  if (tags.size > 0 && !(image.tags || []).some((tag) => tags.has(String(tag).toLowerCase()))) {
    return false;
  }
  return true;
}

function updatePublishCount() {
  const { pictures, outputs } = publishSelectionStats();
  els.publishCount.textContent = `${pictures} ${plural(pictures, "picture")} selected, ${outputs} ${plural(outputs, "output")} will be exported.`;
}

function splitPublishTags(raw) {
  return raw
    .split(/[,\s]+/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function numberOrNull(value) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : null;
}

function publishWouldRerender() {
  const defaults = publishDefaults();
  if (!defaults || !els.publishOutputFormat) return false;
  const body = publishFormBody();
  const defaultsSizeMode = defaults.resize
    ? "geometry"
    : defaults.long_edge
      ? "long-edge"
      : defaults.max_width || defaults.max_height
        ? "bounds"
        : "original";
  const defaultNormalizeGrain = defaults.normalize_grain_mpix !== null;
  const defaultNormalizeGrainMpix = Number(defaults.normalize_grain_mpix ?? 12);
  return (
    body.output_format !== (defaults.output_format || "jpg") ||
    body.grain_engine !== (defaults.grain_engine || "legacy") ||
    body.normalize_grain !== defaultNormalizeGrain ||
    (body.normalize_grain && body.normalize_grain_mpix !== defaultNormalizeGrainMpix) ||
    body.size_mode !== defaultsSizeMode ||
    body.jpg_quality !== Number(defaults.jpg_quality || 95) ||
    body.jpeg_subsampling !== (defaults.jpeg_subsampling || "s444") ||
    Boolean(body.strip_metadata) !== Boolean(defaults.strip_metadata) ||
    Boolean(body.progressive_jpeg) !== Boolean(defaults.progressive_jpeg) ||
    (body.resize || "") !== (defaults.resize || "") ||
    (body.long_edge || null) !== (defaults.long_edge || null) ||
    (body.max_width || null) !== (defaults.max_width || null) ||
    (body.max_height || null) !== (defaults.max_height || null)
  );
}

function updatePublishModeText() {
  const rerender = publishWouldRerender();
  els.publishMode.textContent = rerender
    ? "Changed output or grain settings will rerender selected pictures from the original RAW files."
    : "Settings match daemon defaults, so publish will hardlink reviewed outputs when possible.";
  updatePublishCount();
  updatePublishStatus();
}

function setActiveReviewButtons(image) {
  document.querySelectorAll(".rating button[data-rating]").forEach((button) => {
    button.classList.toggle("active", Number(image?.rating || 0) === Number(button.dataset.rating));
  });
  const labels = new Set(imageLabels(image));
  document.querySelectorAll(".labels button[data-label]").forEach((button) => {
    button.classList.toggle("active", labels.has(button.dataset.label));
  });
}

function imageLabels(image) {
  if (Array.isArray(image?.labels) && image.labels.length > 0) {
    return image.labels.filter((label) => label && label !== "none");
  }
  return image?.label && image.label !== "none" ? [image.label] : [];
}

function labelLetter(label) {
  return { red: "R", yellow: "Y", green: "G", blue: "B", purple: "P" }[label] || "";
}

function LabelBadges({ labels }) {
  return h(
    "span",
    {
      class: "label-badges",
      title: labels.join(", "),
      "aria-label": labels.join(", "),
    },
    labels.map((label) =>
      h(
        "span",
        {
          key: label,
          class: "label-badge",
          "data-label": label,
        },
        labelLetter(label),
      ),
    ),
  );
}

function currentTags() {
  return els.tags.value
    .split(/[,\s]+/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function defaultRetouch() {
  return {
    adjustments: {
      exposure: 0,
      contrast: 0,
      highlights: 0,
      shadows: 0,
      whites: 0,
      blacks: 0,
      temperature: 0,
      offset: 0,
      clarity: 0,
    },
    crop: null,
    rotation_degrees: 0,
  };
}

function normalizedRetouch(retouch) {
  const normalized = retouch || defaultRetouch();
  const crop = normalized.crop
    ? {
        x: clamp(Number(normalized.crop.x) || 0, 0, 1),
        y: clamp(Number(normalized.crop.y) || 0, 0, 1),
        width: clamp(Number(normalized.crop.width) || 1, 0.01, 1),
        height: clamp(Number(normalized.crop.height) || 1, 0.01, 1),
      }
    : null;
  if (crop) {
    crop.x = clamp(crop.x, 0, 1 - crop.width);
    crop.y = clamp(crop.y, 0, 1 - crop.height);
  }
  return {
    adjustments: {
      exposure: clamp(Number(normalized.adjustments?.exposure) || 0, -4, 4),
      contrast: clamp(Number(normalized.adjustments?.contrast) || 0, -100, 100),
      highlights: clamp(Number(normalized.adjustments?.highlights) || 0, -100, 100),
      shadows: clamp(Number(normalized.adjustments?.shadows) || 0, -100, 100),
      whites: clamp(Number(normalized.adjustments?.whites) || 0, -100, 100),
      blacks: clamp(Number(normalized.adjustments?.blacks) || 0, -100, 100),
      temperature: clamp(
        Number(normalized.adjustments?.temperature) || 0,
        -RETOUCH_TEMPERATURE_DELTA_LIMIT,
        RETOUCH_TEMPERATURE_DELTA_LIMIT,
      ),
      offset: clamp(
        Number(normalized.adjustments?.offset) || 0,
        -RETOUCH_OFFSET_DELTA_LIMIT,
        RETOUCH_OFFSET_DELTA_LIMIT,
      ),
      clarity: clamp(Number(normalized.adjustments?.clarity) || 0, -100, 100),
    },
    crop,
    rotation_degrees: normalizeRotation(Number(normalized.rotation_degrees) || 0),
  };
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function normalizeRotation(value) {
  let rotation = Number.isFinite(value) ? value % 360 : 0;
  if (rotation > 180) rotation -= 360;
  if (rotation < -180) rotation += 360;
  return Math.abs(rotation) < 0.0001 ? 0 : rotation;
}

// The UI edits effective profile/as-shot values while persisted retouch state remains portable deltas.
function profileRetouchBase(image = findImage(state.currentId)) {
  const selected = selectedProfile(image);
  const profile = profileByIndex(profileRenderIndex(selected));
  return normalizedRetouch({ adjustments: profile?.retouch_base || {} }).adjustments;
}

function retouchTonalInputValues(image, retouch) {
  const adjustments = normalizedRetouch(retouch).adjustments;
  const base = profileRetouchBase(image);
  return {
    exposure: clamp(base.exposure + adjustments.exposure, -4, 4),
    contrast: clamp(base.contrast + adjustments.contrast, -100, 100),
    highlights: clamp(base.highlights + adjustments.highlights, -100, 100),
    shadows: clamp(base.shadows + adjustments.shadows, -100, 100),
    whites: clamp(base.whites + adjustments.whites, -100, 100),
    blacks: clamp(base.blacks + adjustments.blacks, -100, 100),
    clarity: clamp(base.clarity + adjustments.clarity, -100, 100),
  };
}

function asShotWhiteBalanceTemperature(image = findImage(state.currentId)) {
  const temperature = Number(image?.exif?.white_balance_temperature);
  return Number.isFinite(temperature) && temperature > 0 ? Math.round(temperature) : null;
}

function retouchTemperatureInputValue(image, temperatureDelta) {
  const asShot = asShotWhiteBalanceTemperature(image);
  return asShot === null ? temperatureDelta : asShot + temperatureDelta;
}

function retouchTemperatureDeltaFromInput(image) {
  const inputValue = Number(els.retouchTemperature.value || 0);
  const asShot = asShotWhiteBalanceTemperature(image);
  return asShot === null ? inputValue : inputValue - asShot;
}

function asShotWhiteBalanceOffset(image = findImage(state.currentId)) {
  const value = image?.exif?.white_balance_offset;
  if (value === null || value === undefined || value === "") return null;
  const offset = Number(value);
  return Number.isFinite(offset) ? Math.round(offset) : null;
}

function retouchOffsetInputValue(image, offsetDelta) {
  const asShot = asShotWhiteBalanceOffset(image);
  return asShot === null ? offsetDelta : asShot + offsetDelta;
}

function retouchOffsetDeltaFromInput(image) {
  const inputValue = Number(els.retouchOffset.value || 0);
  const asShot = asShotWhiteBalanceOffset(image);
  return asShot === null ? inputValue : inputValue - asShot;
}

function retouchFromInputs(image = findImage(state.currentId)) {
  const existing = normalizedRetouch(image?.retouch || defaultRetouch());
  const base = profileRetouchBase(image);
  return retouchForImage(image, {
    adjustments: {
      exposure: Number(els.retouchExposure.value || 0) - base.exposure,
      contrast: Number(els.retouchContrast.value || 0) - base.contrast,
      highlights: Number(els.retouchHighlights.value || 0) - base.highlights,
      shadows: Number(els.retouchShadows.value || 0) - base.shadows,
      whites: Number(els.retouchWhites.value || 0) - base.whites,
      blacks: Number(els.retouchBlacks.value || 0) - base.blacks,
      temperature: retouchTemperatureDeltaFromInput(image),
      offset: retouchOffsetDeltaFromInput(image),
      clarity: Number(els.retouchClarity.value || 0) - base.clarity,
    },
    crop: existing.crop,
    rotation_degrees: existing.rotation_degrees,
  });
}

function retouchForImage(image, retouch) {
  const normalized = normalizedRetouch(retouch);
  if (!isDirectCompressedImage(image)) return normalized;
  return normalizedRetouch({
    ...normalized,
    adjustments: defaultRetouch().adjustments,
  });
}

function cloneRetouch(retouch) {
  return normalizedRetouch(JSON.parse(JSON.stringify(retouch || defaultRetouch())));
}

function cloneRetouchAdjustments(retouch) {
  return cloneRetouch(retouch).adjustments;
}

function setRetouchInputs(retouch, image = findImage(state.currentId)) {
  const normalized = normalizedRetouch(retouch);
  const base = profileRetouchBase(image);
  const tonalValues = retouchTonalInputValues(image, normalized);
  const asShotTemperature = asShotWhiteBalanceTemperature(image);
  const asShotOffset = asShotWhiteBalanceOffset(image);
  els.retouchExposure.defaultValue = String(base.exposure);
  els.retouchContrast.defaultValue = String(base.contrast);
  els.retouchHighlights.defaultValue = String(base.highlights);
  els.retouchShadows.defaultValue = String(base.shadows);
  els.retouchWhites.defaultValue = String(base.whites);
  els.retouchBlacks.defaultValue = String(base.blacks);
  els.retouchClarity.defaultValue = String(base.clarity);
  els.retouchExposure.value = String(tonalValues.exposure);
  els.retouchContrast.value = String(tonalValues.contrast);
  els.retouchHighlights.value = String(tonalValues.highlights);
  els.retouchShadows.value = String(tonalValues.shadows);
  els.retouchWhites.value = String(tonalValues.whites);
  els.retouchBlacks.value = String(tonalValues.blacks);
  if (asShotTemperature === null) {
    els.retouchTemperature.min = String(-RETOUCH_TEMPERATURE_DELTA_LIMIT);
    els.retouchTemperature.max = String(RETOUCH_TEMPERATURE_DELTA_LIMIT);
    els.retouchTemperature.defaultValue = "0";
  } else {
    els.retouchTemperature.min = String(asShotTemperature - RETOUCH_TEMPERATURE_DELTA_LIMIT);
    els.retouchTemperature.max = String(asShotTemperature + RETOUCH_TEMPERATURE_DELTA_LIMIT);
    els.retouchTemperature.defaultValue = String(asShotTemperature);
  }
  els.retouchTemperature.value = String(retouchTemperatureInputValue(image, normalized.adjustments.temperature));
  els.retouchTemperatureLabel.title = image?.exif?.white_balance_mode
    ? `White balance: ${image.exif.white_balance_mode}`
    : "Double-click to reset";
  if (asShotOffset === null) {
    els.retouchOffset.min = String(-RETOUCH_OFFSET_DELTA_LIMIT);
    els.retouchOffset.max = String(RETOUCH_OFFSET_DELTA_LIMIT);
    els.retouchOffset.defaultValue = "0";
  } else {
    els.retouchOffset.min = String(asShotOffset - RETOUCH_OFFSET_DELTA_LIMIT);
    els.retouchOffset.max = String(asShotOffset + RETOUCH_OFFSET_DELTA_LIMIT);
    els.retouchOffset.defaultValue = String(asShotOffset);
  }
  els.retouchOffset.value = String(retouchOffsetInputValue(image, normalized.adjustments.offset));
  els.retouchClarity.value = String(tonalValues.clarity);
  updateRetouchReadouts(normalized, image);
}

function updateRetouchReadouts(retouch = retouchFromInputs(), image = findImage(state.currentId)) {
  const normalized = normalizedRetouch(retouch);
  const tonalValues = retouchTonalInputValues(image, normalized);
  els.retouchExposureValue.value = signed(tonalValues.exposure, 2);
  els.retouchContrastValue.value = signed(tonalValues.contrast, 0);
  els.retouchHighlightsValue.value = signed(tonalValues.highlights, 0);
  els.retouchShadowsValue.value = signed(tonalValues.shadows, 0);
  els.retouchWhitesValue.value = signed(tonalValues.whites, 0);
  els.retouchBlacksValue.value = signed(tonalValues.blacks, 0);
  const temperature = Math.round(retouchTemperatureInputValue(image, normalized.adjustments.temperature));
  els.retouchTemperatureValue.value = `${asShotWhiteBalanceTemperature(image) === null ? signed(temperature, 0) : temperature}K`;
  const offset = Math.round(retouchOffsetInputValue(image, normalized.adjustments.offset));
  els.retouchOffsetValue.value = signed(offset, 0);
  els.retouchClarityValue.value = signed(tonalValues.clarity, 0);
}

function signed(value, digits) {
  const rounded = Number(value || 0).toFixed(digits);
  return Number(rounded) > 0 ? `+${rounded}` : rounded;
}

function isRetouchControlActive() {
  return Boolean(document.activeElement?.closest(".retouch"));
}

function clearActiveRetouchSlider() {
  const active = state.retouchActiveSliderId ? document.getElementById(state.retouchActiveSliderId) : null;
  active?.closest("label")?.classList.remove("retouch-slider-active");
  state.retouchActiveSliderId = null;
  state.retouchActiveSliderOriginalValue = null;
}

function setActiveRetouchSlider(input) {
  clearActiveRetouchSlider();
  if (!input) return;
  state.retouchActiveSliderId = input?.id || null;
  state.retouchActiveSliderOriginalValue = input.value;
  input?.closest("label")?.classList.add("retouch-slider-active");
}

function activeRetouchSlider() {
  return state.retouchActiveSliderId ? document.getElementById(state.retouchActiveSliderId) : null;
}

function parseSliderRange(input) {
  const min = Number(input.min);
  const max = Number(input.max);
  const step = Number(input.step);
  return {
    min,
    max,
    step: Number.isFinite(step) && step > 0 ? step : 1,
    valid: Number.isFinite(min) && Number.isFinite(max) && max > min,
  };
}

function nudgeRetouchSlider(input, direction, shiftMode) {
  if (!input) return;
  const { min, max, step, valid } = parseSliderRange(input);
  if (!valid) return;
  const current = Number(input.value);
  if (!Number.isFinite(current)) return;
  const range = max - min;
  const percent = shiftMode ? 0.01 : 0.1;
  const next = clamp(current + range * percent * direction, min, max);
  const snapped = clamp(Math.round((next - min) / step) * step + min, min, max);
  input.value = String(Number(snapped.toFixed(6)));
  const retouch = retouchFromInputs();
  updateRetouchReadouts(retouch);
  applyLocalRetouch(retouch);
  scheduleRetouchSave();
}

function revertActiveRetouchSlider() {
  const input = activeRetouchSlider();
  if (!input) return;
  if (state.retouchActiveSliderOriginalValue === null) {
    clearActiveRetouchSlider();
    input.blur();
    return;
  }
  if (input.value !== state.retouchActiveSliderOriginalValue) {
    input.value = state.retouchActiveSliderOriginalValue;
    clearRetouchSaveTimer();
    const retouch = retouchFromInputs();
    updateRetouchReadouts(retouch);
    applyLocalRetouch(retouch, { save: false });
  }
  clearActiveRetouchSlider();
  input.blur();
}

function commitActiveRetouchSlider() {
  const input = activeRetouchSlider();
  if (!input) return;
  clearRetouchSaveTimer();
  clearActiveRetouchSlider();
  input.blur();
  saveReview({ retouch: retouchFromInputs() }).catch((error) => console.error(error));
}

function maybeClearRetouchSliderActivation(event) {
  const input = activeRetouchSlider();
  if (!input) return;
  const target = event.target;
  if (!(target instanceof Element)) return;
  if (target.closest(".retouch label")) return;
  clearActiveRetouchSlider();
  input.blur();
}

function applyLocalRetouch(retouch, options = {}) {
  const image = findImage(state.currentId);
  if (!image) return;
  state.localRetouchDirty = true;
  image.retouch = retouchForImage(image, retouch);
  setRetouchInputs(image.retouch, image);
  applyDraftRetouch(image, selectedProfile(image));
  scheduleHistogramRender({ debounce: true });
  renderRetouchGrid(image, selectedProfile(image));
  renderFocusOverlay(image);
  renderCropOverlay(image);
  renderList(filteredImages());
  renderProfiles(image);
  const selected = selectedProfile(image);
  renderProfileStateSummary(
    image,
    selected,
    isDirectCompressedImage(image) ? compressedDisplayState(image) : profileDisplayState(image, selected),
    "",
    "",
    isDirectCompressedImage(image) || profilesAreImplicitOnly(image),
  );
  if (options.save !== false) scheduleRetouchSave();
}

function copyCurrentRetouch() {
  const image = findImage(state.currentId);
  if (!image || isRetouchControlsDisabledForImage(image)) return;
  state.retouchClipboard = cloneRetouchAdjustments(retouchFromInputs(image));
  syncRetouchClipboardButtons();
  showGestureFeedback("copied sliders");
}

function pasteCurrentRetouch() {
  const image = findImage(state.currentId);
  if (!image || isRetouchControlsDisabledForImage(image) || !state.retouchClipboard) return false;
  const current = retouchFromInputs(image);
  const retouch = retouchForImage(image, {
    ...current,
    adjustments: state.retouchClipboard,
  });
  applyLocalRetouch(retouch, { save: false });
  saveReview({ retouch }).catch((error) => console.error(error));
  showGestureFeedback("pasted sliders");
  return true;
}

function syncRetouchClipboardButtons() {
  const image = findImage(state.currentId);
  els.retouchPaste.disabled = isRetouchControlsDisabledForImage(image) || !state.retouchClipboard;
}

function applyDraftRetouch(image, selected) {
  const retouch = retouchForImage(image, image?.retouch || defaultRetouch());
  if (isSoocProfile(selected)) {
    retouch.adjustments = defaultRetouch().adjustments;
  }
  const pending = state.localRetouchDirty || (selected && selected.status !== "done");
  const active = (pending || cropDraftIsFor(image)) && !retouchIsDefault(retouch);
  els.viewer.classList.toggle("draft-retouch", active);
  if (!active) {
    els.image.style.removeProperty("--draft-rotation");
    els.image.style.removeProperty("--draft-brightness");
    els.image.style.removeProperty("--draft-contrast");
    els.image.style.removeProperty("--draft-saturation");
    els.image.style.removeProperty("--draft-sepia");
    els.image.style.removeProperty("--draft-hue");
    return;
  }
  const exposure = retouch.adjustments.exposure;
  const highlights = retouch.adjustments.highlights;
  const shadows = retouch.adjustments.shadows;
  const whites = retouch.adjustments.whites;
  const blacks = retouch.adjustments.blacks;
  const temperature = retouch.adjustments.temperature;
  const offset = retouch.adjustments.offset;
  const clarity = retouch.adjustments.clarity;
  const globalContrast = retouch.adjustments.contrast;
  const brightness = clamp(
    1 + exposure * 0.13 + whites * 0.002 - blacks * 0.0015 + shadows * 0.0015 - highlights * 0.0008,
    0.45,
    1.85,
  );
  const contrast = clamp(1 + globalContrast * 0.004 + clarity * 0.002 + (highlights - shadows) * 0.0008, 0.55, 1.65);
  const saturation = clamp(
    1 + clarity * 0.0015 + Math.abs(temperature) * 0.000015 + Math.abs(offset) * 0.0006,
    0.7,
    1.3,
  );
  const sepia = clamp(Math.max(0, temperature) / 2500, 0, 1) * 0.12;
  const hue = clamp(-temperature / 2500, -1, 1) * 5 + clamp(offset / 100, -1, 1) * 4;
  els.image.style.setProperty("--draft-rotation", `${retouch.rotation_degrees}deg`);
  els.image.style.setProperty("--draft-brightness", brightness.toFixed(3));
  els.image.style.setProperty("--draft-contrast", contrast.toFixed(3));
  els.image.style.setProperty("--draft-saturation", saturation.toFixed(3));
  els.image.style.setProperty("--draft-sepia", sepia.toFixed(3));
  els.image.style.setProperty("--draft-hue", `${hue.toFixed(3)}deg`);
}

function retouchIsDefault(retouch) {
  const normalized = normalizedRetouch(retouch);
  return (
    normalized.adjustments.exposure === 0 &&
    normalized.adjustments.contrast === 0 &&
    normalized.adjustments.highlights === 0 &&
    normalized.adjustments.shadows === 0 &&
    normalized.adjustments.whites === 0 &&
    normalized.adjustments.blacks === 0 &&
    normalized.adjustments.temperature === 0 &&
    normalized.adjustments.offset === 0 &&
    normalized.adjustments.clarity === 0 &&
    normalized.rotation_degrees === 0 &&
    !normalized.crop
  );
}

function renderRetouchGrid(image, selected = selectedProfile(image)) {
  if (cropDraftIsFor(image)) {
    els.retouchGrid.hidden = true;
    return;
  }
  const retouch = retouchForImage(image, image?.retouch || defaultRetouch());
  const display = profileDisplayState(image, selected);
  const rotating =
    Math.abs(retouch.rotation_degrees) > 0.001 &&
    (state.localRetouchDirty || display.state === "retouch-queued" || display.state === "retouch-processing");
  els.retouchGrid.hidden = !rotating;
  if (rotating) positionRetouchGrid();
}

function positionRetouchGrid() {
  const imageRect = els.image.getBoundingClientRect();
  const viewerRect = els.viewer.getBoundingClientRect();
  if (imageRect.width <= 1 || imageRect.height <= 1) return;
  els.retouchGrid.style.left = `${imageRect.left - viewerRect.left}px`;
  els.retouchGrid.style.top = `${imageRect.top - viewerRect.top}px`;
  els.retouchGrid.style.width = `${imageRect.width}px`;
  els.retouchGrid.style.height = `${imageRect.height}px`;
}

function toggleInformation(force) {
  state.informationOpen = force ?? !state.informationOpen;
  renderFocusOverlay(findImage(state.currentId));
}

function renderFocusOverlay(image = findImage(state.currentId)) {
  els.focusOverlay.replaceChildren();
  const selected = selectedProfile(image);
  const renderPending =
    state.localRetouchDirty || Boolean(image?.preview_retouch_pending) || Boolean(selected?.retouch_pending);
  const visible =
    state.informationOpen &&
    image &&
    !state.cropEditing &&
    !renderPending &&
    els.image.complete &&
    els.image.naturalWidth > 0 &&
    els.image.naturalHeight > 0;
  if (!visible) {
    els.focusOverlay.setAttribute("hidden", "");
    return;
  }

  const polygons = focusRegionPolygons(image);
  if (polygons.length === 0) {
    els.focusOverlay.setAttribute("hidden", "");
    return;
  }
  for (const polygon of polygons) {
    const element = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
    element.setAttribute(
      "points",
      polygon.points.map((point) => `${(point.x * 1000).toFixed(3)},${(point.y * 1000).toFixed(3)}`).join(" "),
    );
    element.setAttribute("class", polygon.primary ? "focus-region focus-region-primary" : "focus-region");
    els.focusOverlay.append(element);
  }
  positionFocusOverlay();
  els.focusOverlay.removeAttribute("hidden");
}

function focusRegionPolygons(image) {
  const frameWidth = Number(image?.exif?.focus_frame_width);
  const frameHeight = Number(image?.exif?.focus_frame_height);
  const regions = image?.exif?.focus_regions || [];
  if (
    !Number.isFinite(frameWidth) ||
    frameWidth <= 0 ||
    !Number.isFinite(frameHeight) ||
    frameHeight <= 0 ||
    regions.length === 0
  ) {
    return [];
  }

  const retouch = retouchForImage(image, image.retouch || defaultRetouch());
  const rotation = normalizeRotation(retouch.rotation_degrees);
  const safe = rotatedSafeDimensions(frameWidth, frameHeight, rotation);
  const crop = retouch.crop || fullFrameCrop();
  const cropLeft = crop.x * safe.width;
  const cropTop = crop.y * safe.height;
  const cropWidth = crop.width * safe.width;
  const cropHeight = crop.height * safe.height;
  const radians = (rotation * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);

  return regions.flatMap((region) => {
    const left = Number(region.x) * frameWidth;
    const top = Number(region.y) * frameHeight;
    const right = (Number(region.x) + Number(region.width)) * frameWidth;
    const bottom = (Number(region.y) + Number(region.height)) * frameHeight;
    if (![left, top, right, bottom].every(Number.isFinite) || right <= left || bottom <= top) return [];
    const points = [
      { x: left, y: top },
      { x: right, y: top },
      { x: right, y: bottom },
      { x: left, y: bottom },
    ].map((point) => {
      const x = point.x - frameWidth / 2;
      const y = point.y - frameHeight / 2;
      return {
        x: (cos * x - sin * y + safe.width / 2 - cropLeft) / cropWidth,
        y: (sin * x + cos * y + safe.height / 2 - cropTop) / cropHeight,
      };
    });
    const xValues = points.map((point) => point.x);
    const yValues = points.map((point) => point.y);
    if (
      Math.max(...xValues) <= 0 ||
      Math.min(...xValues) >= 1 ||
      Math.max(...yValues) <= 0 ||
      Math.min(...yValues) >= 1
    ) {
      return [];
    }
    return [{ points, primary: Boolean(region.primary) }];
  });
}

function positionFocusOverlay() {
  const imageRect = els.image.getBoundingClientRect();
  const viewerRect = els.viewer.getBoundingClientRect();
  if (imageRect.width <= 1 || imageRect.height <= 1) return;
  els.focusOverlay.style.left = `${imageRect.left - viewerRect.left}px`;
  els.focusOverlay.style.top = `${imageRect.top - viewerRect.top}px`;
  els.focusOverlay.style.width = `${imageRect.width}px`;
  els.focusOverlay.style.height = `${imageRect.height}px`;
}

function renderCropOverlay(image) {
  const crop = cropForOverlay(image);
  const editing = cropDraftIsFor(image);
  const ready = editing && state.cropSourceReady;
  const visible = Boolean(image && ready && state.cropGeometryInitialized && crop);
  els.cropStage.hidden = !ready;
  els.cropOverlay.hidden = !visible;
  els.cropBox.hidden = !visible;
  els.cropTools.hidden = !editing;
  els.cropRatio.disabled = !state.cropGeometryInitialized;
  els.app.classList.toggle("crop-mode", ready);
  updateCropButtons(image);
  updateCropRotationControls();
  updateCropRatioControls();
  if (!visible) {
    return;
  }
  positionCropOverlay();
  els.cropBox.style.left = `${crop.x * 100}%`;
  els.cropBox.style.top = `${crop.y * 100}%`;
  els.cropBox.style.width = `${crop.width * 100}%`;
  els.cropBox.style.height = `${crop.height * 100}%`;
}

function positionCropOverlay() {
  layoutCropStage();
}

function defaultCrop() {
  return { x: 0.1, y: 0.1, width: 0.8, height: 0.8 };
}

function fullFrameCrop() {
  return { x: 0, y: 0, width: 1, height: 1 };
}

function cropDraftIsFor(image) {
  return Boolean(image && state.cropEditing && state.cropDraftImageId === image.id);
}

function cropForOverlay(image) {
  if (!image) return null;
  if (cropDraftIsFor(image)) return state.cropDraft || fullFrameCrop();
  return null;
}

function hasCropAdjustment(image) {
  if (!image) return false;
  const retouch = normalizedRetouch(image.retouch || defaultRetouch());
  return Boolean(retouch.crop) || Math.abs(retouch.rotation_degrees) > 0.001;
}

function cropEditingSource(image, selected) {
  if (image?.crop_source_url) {
    return { url: image.crop_source_url, updatedAt: image.crop_source_updated_at || image.preview_updated_at };
  }
  if (selected?.base_url) return { url: selected.base_url, updatedAt: selected.updated_at };
  if (image?.preview_url) return { url: image.preview_url, updatedAt: image.preview_updated_at };
  if (selected?.url) return { url: selected.url, updatedAt: selected.updated_at };
  return { url: null, updatedAt: null };
}

function setCropImageSource(element, source) {
  if (!source?.url) {
    element.removeAttribute("src");
    return;
  }
  const url = versionedUrl(source.url, source.updatedAt);
  if (element.getAttribute("src") !== url) element.src = url;
}

function cropSourceDimensions() {
  const naturalWidth = els.cropSourceImage.naturalWidth;
  const naturalHeight = els.cropSourceImage.naturalHeight;
  if (naturalWidth < 1 || naturalHeight < 1) return null;
  const image = findImage(state.cropDraftImageId);
  let width = Number(image?.source_width);
  let height = Number(image?.source_height);
  if (!Number.isFinite(width) || width < 1 || !Number.isFinite(height) || height < 1) {
    return { width: naturalWidth, height: naturalHeight };
  }
  if (width > height !== naturalWidth > naturalHeight) {
    [width, height] = [height, width];
  }
  return { width, height };
}

function isQuarterTurn(rotation) {
  return Math.abs(Math.abs(normalizeRotation(rotation)) - 90) < 0.001;
}

function isHalfTurn(rotation) {
  return Math.abs(Math.abs(normalizeRotation(rotation)) - 180) < 0.001;
}

function rotatedSafeDimensions(width, height, rotation) {
  const normalized = normalizeRotation(rotation);
  if (Math.abs(normalized) < 0.001 || isHalfTurn(normalized)) return { width, height };
  if (isQuarterTurn(normalized)) return { width: height, height: width };

  const radians = (Math.abs(normalized) * Math.PI) / 180;
  const sin = Math.abs(Math.sin(radians));
  const cos = Math.abs(Math.cos(radians));
  const longSide = Math.max(width, height);
  const shortSide = Math.min(width, height);
  let safeWidth;
  let safeHeight;
  if (shortSide <= 2 * sin * cos * longSide || Math.abs(sin - cos) < Number.EPSILON) {
    const side = 0.5 * shortSide;
    if (width >= height) {
      safeWidth = side / sin;
      safeHeight = side / cos;
    } else {
      safeWidth = side / cos;
      safeHeight = side / sin;
    }
  } else {
    const cos2 = cos * cos - sin * sin;
    safeWidth = (width * cos - height * sin) / cos2;
    safeHeight = (height * cos - width * sin) / cos2;
  }
  return {
    width: Math.max(1, Math.min(width, Math.floor(safeWidth))),
    height: Math.max(1, Math.min(height, Math.floor(safeHeight))),
  };
}

function cropViewerSpace() {
  const style = getComputedStyle(els.viewer);
  const horizontal = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
  const vertical = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
  return {
    width: Math.max(1, els.viewer.clientWidth - horizontal),
    height: Math.max(1, els.viewer.clientHeight - vertical),
  };
}

function layoutCropStage() {
  if (!state.cropEditing || !state.cropSourceReady) return;
  const source = cropSourceDimensions();
  if (!source) return;
  const safe = rotatedSafeDimensions(source.width, source.height, state.cropDraftRotation);
  const available = cropViewerSpace();
  const scale = Math.min(available.width / safe.width, available.height / safe.height);
  const layout = { source, safe, scale };
  state.cropLayout = layout;
  els.cropStage.style.width = `${safe.width * scale}px`;
  els.cropStage.style.height = `${safe.height * scale}px`;
  els.cropSourceImage.style.width = `${source.width * scale}px`;
  els.cropSourceImage.style.height = `${source.height * scale}px`;
  els.cropSourceImage.style.transform = `translate(-50%, -50%) rotate(${state.cropDraftRotation}deg)`;
  layoutSavedCropLayer(layout);
}

function layoutSavedCropLayer(layout) {
  const saved = state.cropSaved;
  if (!saved?.currentUrl) {
    els.cropCurrentImage.hidden = true;
    return;
  }
  const oldSafe = rotatedSafeDimensions(layout.source.width, layout.source.height, saved.rotation);
  const crop = saved.crop || fullFrameCrop();
  const oldCenterX = (crop.x + crop.width / 2) * oldSafe.width - oldSafe.width / 2;
  const oldCenterY = (crop.y + crop.height / 2) * oldSafe.height - oldSafe.height / 2;
  const delta = normalizeRotation(state.cropDraftRotation - saved.rotation);
  const radians = (delta * Math.PI) / 180;
  const centerX = Math.cos(radians) * oldCenterX - Math.sin(radians) * oldCenterY;
  const centerY = Math.sin(radians) * oldCenterX + Math.cos(radians) * oldCenterY;
  const width = crop.width * oldSafe.width * layout.scale;
  const height = crop.height * oldSafe.height * layout.scale;
  els.cropCurrentImage.hidden = false;
  els.cropCurrentImage.style.left = `${(layout.safe.width / 2 + centerX) * layout.scale - width / 2}px`;
  els.cropCurrentImage.style.top = `${(layout.safe.height / 2 + centerY) * layout.scale - height / 2}px`;
  els.cropCurrentImage.style.width = `${width}px`;
  els.cropCurrentImage.style.height = `${height}px`;
  els.cropCurrentImage.style.transform = `rotate(${delta}deg)`;
}

function initializeCropGeometry() {
  const image = findImage(state.currentId);
  if (!cropDraftIsFor(image) || !cropSourceDimensions()) return;
  state.cropSourceReady = true;
  state.cropGeometryInitialized = true;
  if (state.cropSaved?.hadCrop) {
    inferCropRatio(state.cropDraft || fullFrameCrop());
  } else {
    state.cropRatioKey = "original";
    state.cropRatioBase = null;
    state.cropRatioRotated = false;
    state.cropDraft = fitCropToRatio(defaultCrop(), cropTargetRatio());
  }
  rememberCropRatioGeometry(state.cropDraft);
  renderCropOverlay(image);
}

function clearCropDraftState() {
  state.cropEditing = false;
  state.cropDraft = null;
  state.cropDraftRotation = 0;
  state.cropDraftImageId = null;
  state.cropSaved = null;
  state.cropSourceReady = false;
  state.cropGeometryInitialized = false;
  state.cropLayout = null;
  state.cropRatioKey = "original";
  state.cropRatioBase = null;
  state.cropRatioRotated = false;
  state.cropRatioGeometry = null;
  state.cropDrag = null;
  state.cropPointers.clear();
  state.cropTouchGesture = null;
  els.cropStage.hidden = true;
  els.cropOverlay.hidden = true;
  els.cropTools.hidden = true;
  els.cropActions.hidden = true;
  els.app.classList.remove("crop-mode");
}

function beginCropEditing() {
  const image = findImage(state.currentId);
  if (!image) return;
  if (cropDraftIsFor(image)) return;
  stopZoom();
  clearRetouchSaveTimer();
  const retouch = normalizedRetouch(image.retouch || defaultRetouch());
  const selected = selectedProfile(image);
  const source = cropEditingSource(image, selected);
  if (!source.url) return;
  const current = selected?.url
    ? { url: selected.url, updatedAt: selected.updated_at }
    : { url: null, updatedAt: null };
  state.cropEditing = true;
  state.cropDraftImageId = image.id;
  state.cropDraft = retouch.crop || defaultCrop();
  state.cropDraftRotation = retouch.rotation_degrees;
  state.cropSaved = {
    crop: retouch.crop || fullFrameCrop(),
    hadCrop: Boolean(retouch.crop),
    rotation: retouch.rotation_degrees,
    currentUrl: current.url ? versionedUrl(current.url, current.updatedAt) : null,
  };
  state.cropSourceReady = false;
  state.cropGeometryInitialized = false;
  els.cropCurrentImage.hidden = !state.cropSaved.currentUrl;
  if (state.cropSaved.currentUrl) els.cropCurrentImage.src = state.cropSaved.currentUrl;
  setCropImageSource(els.cropSourceImage, source);
  updateCropRotationControls();
  applyDraftRetouch(image, selected);
  renderRetouchGrid(image, selected);
  renderFocusOverlay(image);
  renderCropOverlay(image);
  if (els.cropSourceImage.complete && els.cropSourceImage.naturalWidth > 0) initializeCropGeometry();
}

function cancelCropEditing() {
  const image = findImage(state.currentId);
  clearCropDraftState();
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image, selectedProfile(image));
  renderFocusOverlay(image);
  renderCropOverlay(image);
}

function approveCropEditing() {
  const image = findImage(state.currentId);
  if (!cropDraftIsFor(image)) return;
  const crop = state.cropDraft ? { ...state.cropDraft } : null;
  const rotation = state.cropDraftRotation;
  clearCropDraftState();
  applyLocalRetouch(
    normalizedRetouch({
      ...retouchFromInputs(image),
      crop,
      rotation_degrees: rotation,
    }),
  );
}

function clearCropDraft() {
  const image = findImage(state.currentId);
  if (!image) return;
  clearCropDraftState();
  applyLocalRetouch(
    normalizedRetouch({
      ...retouchFromInputs(image),
      crop: null,
      rotation_degrees: 0,
    }),
  );
}

function updateCropButtons(image) {
  const editing = cropDraftIsFor(image);
  const adjusted = editing || hasCropAdjustment(image);
  els.cropToggle.classList.toggle("active", adjusted);
  els.cropToggle.title = adjusted ? "Crop or rotation adjustment active" : "Crop/rotate";
  els.cropActions.hidden = !editing;
}

function updateCropRotationControls() {
  els.cropRotation.value = String(clamp(state.cropDraftRotation, -180, 180));
  els.cropRotationValue.value = `${signed(state.cropDraftRotation, 1)}°`;
}

function cropRatioBase(key = state.cropRatioKey) {
  if (key === "free") return null;
  if (key === "current") return state.cropRatioBase || 1;
  if (key === "a3-a4") return Math.SQRT2;
  if (key === "original") {
    const source = cropSourceDimensions();
    return source ? source.width / source.height : 1;
  }
  const [width, height] = key.split(":").map(Number);
  return width > 0 && height > 0 ? width / height : 1;
}

function cropTargetRatio() {
  const base = cropRatioBase();
  if (base === null) return null;
  return state.cropRatioRotated ? 1 / base : base;
}

function cropPixelRatio(crop, rotation = state.cropDraftRotation) {
  const source = cropSourceDimensions();
  if (!source || !crop) return 1;
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  return (crop.width * safe.width) / Math.max(1, crop.height * safe.height);
}

function ratiosMatch(left, right) {
  if (!Number.isFinite(left) || !Number.isFinite(right)) return false;
  return Math.abs(Math.log(Math.max(0.0001, left) / Math.max(0.0001, right))) < 0.004;
}

function inferCropRatio(crop) {
  const actual = cropPixelRatio(crop);
  for (const [key] of CROP_RATIO_PRESETS) {
    if (key === "free") continue;
    const base = cropRatioBase(key);
    if (ratiosMatch(actual, base)) {
      state.cropRatioKey = key;
      state.cropRatioBase = null;
      state.cropRatioRotated = false;
      return;
    }
    if (ratiosMatch(actual, 1 / base)) {
      state.cropRatioKey = key;
      state.cropRatioBase = null;
      state.cropRatioRotated = true;
      return;
    }
  }
  state.cropRatioKey = "free";
  state.cropRatioBase = null;
  state.cropRatioRotated = false;
}

function formatCropRatio(ratio) {
  let bestNumerator = ratio;
  let bestDenominator = 1;
  let bestError = Infinity;
  for (let denominator = 1; denominator <= 20; denominator += 1) {
    const numerator = Math.max(1, Math.round(ratio * denominator));
    const error = Math.abs(ratio - numerator / denominator);
    if (error < bestError) {
      bestNumerator = numerator;
      bestDenominator = denominator;
      bestError = error;
    }
  }
  return `${bestNumerator}:${bestDenominator}`;
}

function updateCropRatioControls() {
  const currentOption = els.cropRatio.querySelector('option[value="current"]');
  currentOption.hidden = state.cropRatioKey !== "current";
  for (const [value, label] of CROP_RATIO_PRESETS) {
    const option = els.cropRatio.querySelector(`option[value="${value}"]`);
    option.textContent = label;
  }
  const selected = els.cropRatio.querySelector(`option[value="${state.cropRatioKey}"]`);
  const selectedPreset = CROP_RATIO_PRESETS.find(([value]) => value === state.cropRatioKey);
  if (selected) {
    if (state.cropRatioKey === "original") {
      selected.textContent = `Original ${formatCropRatio(cropTargetRatio())}`;
    } else if (state.cropRatioKey === "current") {
      selected.textContent = `Current ${formatCropRatio(cropTargetRatio())}`;
    } else if (state.cropRatioRotated && state.cropRatioKey !== "1:1") {
      selected.textContent = selectedPreset?.[2] || state.cropRatioKey.split(":").reverse().join(":");
    }
  }
  els.cropRatio.value = state.cropRatioKey;
}

function normalizeCropRect(crop) {
  const requestedWidth = Number(crop?.width);
  const requestedHeight = Number(crop?.height);
  const requestedX = Number(crop?.x);
  const requestedY = Number(crop?.y);
  const width = clamp(Number.isFinite(requestedWidth) ? requestedWidth : 1, 0.01, 1);
  const height = clamp(Number.isFinite(requestedHeight) ? requestedHeight : 1, 0.01, 1);
  return {
    x: clamp(Number.isFinite(requestedX) ? requestedX : 0, 0, 1 - width),
    y: clamp(Number.isFinite(requestedY) ? requestedY : 0, 0, 1 - height),
    width,
    height,
  };
}

function cropRectAround(center, width, height) {
  const normalizedWidth = clamp(width, 0.01, 1);
  const normalizedHeight = clamp(height, 0.01, 1);
  return normalizeCropRect({
    x: clamp(center.x, 0, 1) - normalizedWidth / 2,
    y: clamp(center.y, 0, 1) - normalizedHeight / 2,
    width: normalizedWidth,
    height: normalizedHeight,
  });
}

function fitCropToRatio(crop, ratio) {
  const source = cropSourceDimensions();
  if (!source || ratio === null || !Number.isFinite(ratio) || ratio <= 0) return normalizeCropRect(crop);
  const rect = normalizeCropRect(crop);
  const safe = rotatedSafeDimensions(source.width, source.height, state.cropDraftRotation);
  let pixelWidth = rect.width * safe.width;
  let pixelHeight = rect.height * safe.height;
  if (pixelWidth / pixelHeight > ratio) {
    pixelWidth = pixelHeight * ratio;
  } else {
    pixelHeight = pixelWidth / ratio;
  }
  return cropRectAround(
    { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 },
    pixelWidth / safe.width,
    pixelHeight / safe.height,
  );
}

function rememberCropRatioGeometry(crop) {
  const source = cropSourceDimensions();
  if (!source || !crop) {
    state.cropRatioGeometry = null;
    return;
  }
  const rect = normalizeCropRect(crop);
  const safe = rotatedSafeDimensions(source.width, source.height, state.cropDraftRotation);
  state.cropRatioGeometry = {
    center: { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 },
    area: rect.width * safe.width * rect.height * safe.height,
  };
}

function cropForRememberedRatio(ratio) {
  const source = cropSourceDimensions();
  const geometry = state.cropRatioGeometry;
  if (!source || !geometry || ratio === null || !Number.isFinite(ratio) || ratio <= 0) {
    return fitCropToRatio(state.cropDraft || fullFrameCrop(), ratio);
  }
  const safe = rotatedSafeDimensions(source.width, source.height, state.cropDraftRotation);
  let pixelWidth = Math.sqrt(geometry.area * ratio);
  let pixelHeight = Math.sqrt(geometry.area / ratio);
  const scale = Math.min(1, safe.width / pixelWidth, safe.height / pixelHeight);
  pixelWidth *= scale;
  pixelHeight *= scale;
  return cropRectAround(geometry.center, pixelWidth / safe.width, pixelHeight / safe.height);
}

function setCropRatioPreset(key) {
  if (!state.cropGeometryInitialized || !CROP_RATIO_PRESETS.some(([value]) => value === key)) return;
  if (!state.cropRatioGeometry) rememberCropRatioGeometry(state.cropDraft);
  state.cropRatioKey = key;
  state.cropRatioBase = null;
  state.cropRatioRotated = false;
  state.cropDraft = cropForRememberedRatio(cropTargetRatio());
  renderCropOverlay(findImage(state.currentId));
}

function rotateCropRatio() {
  if (!state.cropGeometryInitialized || cropTargetRatio() === null || ratiosMatch(cropTargetRatio(), 1)) return;
  if (!state.cropRatioGeometry) rememberCropRatioGeometry(state.cropDraft);
  state.cropRatioRotated = !state.cropRatioRotated;
  state.cropDraft = cropForRememberedRatio(cropTargetRatio());
  renderCropOverlay(findImage(state.currentId));
}

function setCropDraftRotation(value) {
  let image = findImage(state.currentId);
  if (!cropDraftIsFor(image)) {
    beginCropEditing();
    image = findImage(state.currentId);
    if (!cropDraftIsFor(image)) return;
  }
  if (state.cropGeometryInitialized && state.cropDraft && !state.cropRatioGeometry) {
    rememberCropRatioGeometry(state.cropDraft);
  }
  state.cropDraftRotation = normalizeRotation(value);
  if (state.cropGeometryInitialized && state.cropDraft) {
    state.cropDraft = cropForRememberedRatio(cropTargetRatio());
  }
  updateCropRotationControls();
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image, selectedProfile(image));
  renderCropOverlay(image);
}

function cropPointer(event) {
  return { x: event.clientX, y: event.clientY };
}

function cropPointerPair() {
  return Array.from(state.cropPointers.values()).slice(0, 2);
}

function cropGestureMetrics(points, rect) {
  const [first, second] = points;
  return {
    center: {
      x: ((first.x + second.x) / 2 - rect.left) / Math.max(1, rect.width),
      y: ((first.y + second.y) / 2 - rect.top) / Math.max(1, rect.height),
    },
    distance: Math.hypot(second.x - first.x, second.y - first.y),
    angle: Math.atan2(second.y - first.y, second.x - first.x),
  };
}

function startCropTouchGesture() {
  const points = cropPointerPair();
  if (points.length < 2) return;
  const rect = els.cropOverlay.getBoundingClientRect();
  const metrics = cropGestureMetrics(points, rect);
  state.cropDrag = null;
  state.cropTouchGesture = {
    rect,
    startDistance: Math.max(1, metrics.distance),
    startAngle: metrics.angle,
    crop: state.cropDraft || fullFrameCrop(),
    rotation: state.cropDraftRotation,
  };
}

function updateCropTouchGesture() {
  const gesture = state.cropTouchGesture;
  const points = cropPointerPair();
  if (!gesture || points.length < 2) return;
  const image = findImage(state.currentId);
  const metrics = cropGestureMetrics(points, gesture.rect);
  const scale = metrics.distance / gesture.startDistance;
  state.cropDraftRotation = normalizeRotation(
    gesture.rotation + ((metrics.angle - gesture.startAngle) * 180) / Math.PI,
  );
  state.cropDraft = fitCropToRatio(
    cropRectAround(metrics.center, gesture.crop.width * scale, gesture.crop.height * scale),
    cropTargetRatio(),
  );
  rememberCropRatioGeometry(state.cropDraft);
  updateCropRotationControls();
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image);
  renderCropOverlay(image);
}

function startCropDrag(event) {
  const image = findImage(state.currentId);
  if (!cropDraftIsFor(image)) return;
  event.preventDefault();
  event.stopPropagation();
  state.cropPointers.set(event.pointerId, cropPointer(event));
  try {
    els.cropBox.setPointerCapture(event.pointerId);
  } catch {
    // Pointer capture can fail if the browser already cancelled the touch.
  }
  if (state.cropPointers.size >= 2) {
    startCropTouchGesture();
    return;
  }
  const rect = els.cropOverlay.getBoundingClientRect();
  state.cropDrag = {
    pointerId: event.pointerId,
    handle: event.target?.dataset?.cropHandle || "move",
    startX: event.clientX,
    startY: event.clientY,
    rect,
    crop: state.cropDraft || fullFrameCrop(),
  };
}

function updateCropDrag(event) {
  if (state.cropPointers.has(event.pointerId)) {
    state.cropPointers.set(event.pointerId, cropPointer(event));
  }
  if (state.cropTouchGesture) {
    event.preventDefault();
    updateCropTouchGesture();
    return;
  }
  if (state.cropPointers.size >= 2) {
    event.preventDefault();
    startCropTouchGesture();
    updateCropTouchGesture();
    return;
  }
  const drag = state.cropDrag;
  if (!drag || drag.pointerId !== event.pointerId) return;
  event.preventDefault();
  const dx = (event.clientX - drag.startX) / Math.max(1, drag.rect.width);
  const dy = (event.clientY - drag.startY) / Math.max(1, drag.rect.height);
  let crop;
  if (drag.handle === "move") {
    crop = normalizeCropRect({
      ...drag.crop,
      x: drag.crop.x + dx,
      y: drag.crop.y + dy,
    });
  } else {
    crop = aspectLockedCrop(drag.crop, drag.handle, dx, dy);
  }
  state.cropDraft = crop;
  rememberCropRatioGeometry(state.cropDraft);
  renderCropOverlay(findImage(state.currentId));
}

function aspectLockedCrop(start, handle, dx, dy) {
  const source = cropSourceDimensions();
  if (!source) return normalizeCropRect(start);
  const safe = rotatedSafeDimensions(source.width, source.height, state.cropDraftRotation);
  const targetRatio = cropTargetRatio();
  const anchorX = handle.includes("w") ? start.x + start.width : start.x;
  const anchorY = handle.includes("n") ? start.y + start.height : start.y;
  const signX = handle.includes("w") ? -1 : 1;
  const signY = handle.includes("n") ? -1 : 1;
  const targetWidth = signX > 0 ? start.width + dx : start.width - dx;
  const targetHeight = signY > 0 ? start.height + dy : start.height - dy;
  if (targetRatio === null) {
    const maxWidth = signX > 0 ? 1 - anchorX : anchorX;
    const maxHeight = signY > 0 ? 1 - anchorY : anchorY;
    const width = clamp(Math.abs(targetWidth), Math.min(0.01, maxWidth), maxWidth);
    const height = clamp(Math.abs(targetHeight), Math.min(0.01, maxHeight), maxHeight);
    return normalizeCropRect({
      x: signX > 0 ? anchorX : anchorX - width,
      y: signY > 0 ? anchorY : anchorY - height,
      width,
      height,
    });
  }
  const normalizedRatio = (targetRatio * safe.height) / safe.width;
  let width = Math.min(Math.abs(targetWidth), Math.abs(targetHeight) * normalizedRatio);
  const maxWidthX = signX > 0 ? 1 - anchorX : anchorX;
  const maxHeight = signY > 0 ? 1 - anchorY : anchorY;
  const maxWidth = Math.min(maxWidthX, maxHeight * normalizedRatio);
  const minWidth = Math.min(maxWidth, Math.max(0.01, normalizedRatio * 0.01));
  width = clamp(width, minWidth, maxWidth);
  const height = width / normalizedRatio;
  return normalizeCropRect({
    x: signX > 0 ? anchorX : anchorX - width,
    y: signY > 0 ? anchorY : anchorY - height,
    width,
    height,
  });
}

function endCropDrag(event) {
  state.cropPointers.delete(event.pointerId);
  if (state.cropTouchGesture && state.cropPointers.size < 2) {
    state.cropTouchGesture = null;
  }
  if (state.cropDrag && state.cropDrag.pointerId === event.pointerId) {
    state.cropDrag = null;
  }
}

async function saveReview(patch = {}) {
  const image = findImage(state.currentId);
  if (!image) return;
  clearRetouchSaveTimer();
  return saveImageReview(image, patch, { useInputs: true });
}

async function saveImageReview(image, patch = {}, options = {}) {
  const body = reviewRequestBody(image, patch, options);
  const pendingProfileIndex = patch.selected_profile_index;
  if (pendingProfileIndex !== undefined) {
    state.pendingProfileSelections.set(image.id, pendingProfileIndex);
    image.selected_profile_index = pendingProfileIndex;
    render();
  }
  state.saveQueue = state.saveQueue
    .catch(() => {})
    .then(async () => {
      const response = await fetch(reviewUrl("api/review"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!response.ok) throw new Error(`review ${response.status}`);
      applyStateMessage(await response.json());
    })
    .catch((error) => {
      if (pendingProfileIndex !== undefined) {
        state.pendingProfileSelections.delete(image.id);
      }
      throw error;
    });
  return state.saveQueue;
}

function reviewRequestBody(image, patch = {}, options = {}) {
  return {
    image_id: image.id,
    rating: patch.rating ?? image.rating,
    label: patch.label ?? (patch.labels ? patch.labels[0] || "none" : image.label || "none"),
    labels: patch.labels ?? imageLabels(image),
    tags: patch.tags ?? (options.useInputs ? currentTags() : image.tags || []),
    notes: patch.notes ?? (options.useInputs ? els.notes.value : image.notes || ""),
    retouch: patch.retouch ?? (options.useInputs ? retouchFromInputs(image) : image.retouch || defaultRetouch()),
    selected_profile_index: patch.selected_profile_index,
    publish_profile_indexes:
      patch.enabled_profile_indexes === undefined
        ? (patch.publish_profile_indexes ?? publishProfileIndexes(image))
        : undefined,
    enabled_profile_indexes: patch.enabled_profile_indexes,
    profile_bw_filters: patch.profile_bw_filters ?? profileBwFilters(image),
    advance_after_update: Boolean(patch.advance_after_update),
  };
}

async function updateSharedUi(patch = {}) {
  const body = {
    current_image_id: patch.current_image_id ?? state.currentId,
    min_rating: patch.min_rating ?? minRating(),
  };
  state.saveQueue = state.saveQueue
    .catch(() => {})
    .then(async () => {
      const response = await fetch(reviewUrl("api/ui"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!response.ok) throw new Error(`review UI ${response.status}`);
      applyStateMessage(await response.json());
    });
  return state.saveQueue;
}

async function updateBurstExpansion(burstId, expanded) {
  const response = await fetch(reviewUrl(`api/bursts/${encodeURIComponent(String(burstId))}`), {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ expanded: Boolean(expanded) }),
  });
  if (!response.ok) throw new Error(`burst ${response.status}`);
  applyStateMessage(await response.json());
}

function saveCurrentIfNeeded() {
  if (state.currentId !== null) {
    clearRetouchSaveTimer();
    return saveReview().catch((error) => console.error(error));
  }
  return Promise.resolve();
}

async function move(delta) {
  const images = filteredImages();
  const index = images.findIndex((image) => image.id === state.currentId);
  if (index < 0) return;
  const next = Math.max(0, Math.min(images.length - 1, index + delta));
  if (next !== index) {
    const carryProfileIndex = selectedProfile(images[index])?.profile_index;
    await saveCurrentIfNeeded();
    await updateSharedUi({ current_image_id: images[next].id, min_rating: minRating() });
    await carrySelectedProfileToImage(images[next].id, carryProfileIndex);
  }
}

async function rateCurrentAndAdvance(rating) {
  const current = findImage(state.currentId);
  const carryProfileIndex = selectedProfile(current)?.profile_index;
  await saveReview({ rating, advance_after_update: true });
  await carrySelectedProfileToImage(state.currentId, carryProfileIndex);
}

async function carrySelectedProfileToImage(imageId, profileIndex) {
  const image = findImage(imageId);
  if (!image || profileIndex === undefined || profileIndex === null) return;
  const profiles = image.profiles || [];
  const publishedProfileIndexes = publishProfileIndexes(image);
  const hasProfile = profiles.some((profile) => profile.profile_index === profileIndex);
  const nextProfileIndex = publishedProfileIndexes.includes(profileIndex) ? profileIndex : publishedProfileIndexes[0];
  if (nextProfileIndex === undefined && !hasProfile) return;
  const resolvedProfileIndex = nextProfileIndex ?? image.selected_profile_index;
  if (resolvedProfileIndex === image.selected_profile_index) return;
  await saveImageReview(image, { selected_profile_index: resolvedProfileIndex });
}

async function selectProfileRelative(delta) {
  const image = findImage(state.currentId);
  if (profilesAreImplicitOnly(image)) return;
  const profiles = (image?.profiles || []).filter((profile) => isSoocProfile(profile) || profile.enabled !== false);
  if (profiles.length === 0) return;
  const index = profiles.findIndex((profile) => profile.profile_index === image.selected_profile_index);
  const next = (Math.max(0, index) + delta + profiles.length) % profiles.length;
  await saveReview({ selected_profile_index: profiles[next].profile_index });
}

async function toggleSelectedProfileAvailability() {
  const image = findImage(state.currentId);
  if (profilesAreImplicitOnly(image)) return;
  const profile = selectedProfile(image);
  if (!image || !profile || isSoocProfile(profile)) return;
  await saveReview({ enabled_profile_indexes: toggleEnabledProfile(image, profile.profile_index) });
}

function toggleCurrentLabel(label) {
  const image = findImage(state.currentId);
  if (!image) return;
  if (label === "none") {
    saveReview({ label: "none", labels: [] });
    return;
  }
  const labels = new Set(imageLabels(image));
  if (labels.has(label)) {
    labels.delete(label);
  } else {
    labels.add(label);
  }
  const nextLabels = ["red", "yellow", "green", "blue", "purple"].filter((candidate) => labels.has(candidate));
  saveReview({ label: nextLabels[0] || "none", labels: nextLabels });
}

function normalizeWheelDelta(event, value) {
  if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) return value * 40;
  if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) return value * window.innerHeight;
  return value;
}

function shouldIgnoreNavigationWheel(event) {
  if (
    !els.publishOverlay.hidden ||
    !els.panoramaOverlay.hidden ||
    !els.samplerOverlay.hidden ||
    !els.diffusionOverlay.hidden ||
    !els.shortcutsOverlay.hidden ||
    state.cropEditing
  )
    return true;
  if (event.ctrlKey || event.metaKey || event.altKey) return true;
  return Boolean(
    event.target.closest("input, textarea, select, .retouch, .crop-tools, .publish-card, .shortcuts-card"),
  );
}

function navigationWheelStep(event) {
  const dx = normalizeWheelDelta(event, event.deltaX);
  const dy = normalizeWheelDelta(event, event.deltaY);
  const axis = Math.abs(dx) > Math.abs(dy) ? "x" : "y";
  const delta = axis === "x" ? dx : dy;
  if (!Number.isFinite(delta) || Math.abs(delta) < 1) return null;

  const now = performance.now();
  if (now < wheelNavigation.lockedUntil) return null;
  if (wheelNavigation.axis !== axis || now - wheelNavigation.lastAt > WHEEL_NAV_RESET_MS) {
    wheelNavigation.axis = axis;
    wheelNavigation.amount = 0;
  }

  wheelNavigation.amount += delta;
  wheelNavigation.lastAt = now;
  if (Math.abs(wheelNavigation.amount) < WHEEL_NAV_THRESHOLD_PX) return null;

  const direction = Math.sign(wheelNavigation.amount);
  wheelNavigation.amount = 0;
  wheelNavigation.lockedUntil = now + WHEEL_NAV_COOLDOWN_MS;
  return { axis, direction };
}

function handleNavigationWheel(event) {
  if (shouldIgnoreNavigationWheel(event)) return;
  event.preventDefault();

  const step = navigationWheelStep(event);
  if (!step) return;

  if (step.axis === "x") {
    move(step.direction > 0 ? 1 : -1).catch((error) => console.error(error));
    return;
  }
  selectProfileRelative(step.direction > 0 ? 1 : -1).catch((error) => console.error(error));
}

function adjustedCurrentRating(delta) {
  const image = findImage(state.currentId);
  const rating = Number(image?.rating || 0) + delta;
  return Math.max(0, Math.min(5, rating));
}

async function rateCurrentWithoutAdvance(rating) {
  await saveReview({ rating });
  showCurrentRatingFeedback();
}

function showCurrentRatingFeedback() {
  const image = findImage(state.currentId);
  showGestureFeedback(String(Number(image?.rating || 0)));
}

function showGestureFeedback(text) {
  clearTimeout(state.gestureFeedbackTimer);
  els.gestureFeedback.textContent = text;
  positionGestureFeedback();
  els.gestureFeedback.hidden = false;
  state.gestureFeedbackTimer = setTimeout(() => {
    els.gestureFeedback.hidden = true;
  }, 850);
}

function positionGestureFeedback() {
  const imageRect = els.image.getBoundingClientRect();
  const viewerRect = els.viewer.getBoundingClientRect();
  if (imageRect.width <= 0 || imageRect.height <= 0 || viewerRect.width <= 0 || viewerRect.height <= 0) {
    els.gestureFeedback.style.left = "50%";
    els.gestureFeedback.style.top = "50%";
    return;
  }
  els.gestureFeedback.style.left = `${imageRect.left - viewerRect.left + imageRect.width / 2}px`;
  els.gestureFeedback.style.top = `${imageRect.top - viewerRect.top + imageRect.height / 2}px`;
}

function pointerTargetElement(event) {
  return event.target instanceof Element ? event.target : null;
}

function isViewerGestureSurface(event) {
  const target = pointerTargetElement(event);
  return Boolean(target && !target.closest(".crop-overlay, .crop-tools, .retouch-grid"));
}

function preventNativeViewerAction(event) {
  if (isViewerGestureSurface(event)) event.preventDefault();
}

function canStartViewerZoom(event) {
  if (state.zoomFullActive || state.cropEditing || !findImage(state.currentId) || !els.image.getAttribute("src"))
    return false;
  if (event.pointerType !== "touch" && event.button !== 0) return false;
  const target = pointerTargetElement(event);
  return !target?.closest(".crop-overlay, .crop-tools, .retouch-grid, .gesture-feedback, .zoom-loupe");
}

function canToggleFullImageZoom(event) {
  if (!window.matchMedia("(hover: hover) and (pointer: fine)").matches) return false;
  if (state.cropEditing || !findImage(state.currentId) || !els.image.getAttribute("src") || event.button !== 0)
    return false;
  const target = pointerTargetElement(event);
  return !target?.closest(".crop-overlay, .crop-tools, .retouch-grid, .gesture-feedback, .zoom-loupe");
}

function startZoomHold(event) {
  if (!canStartViewerZoom(event)) return false;
  cancelZoomHold();
  state.zoomLastPoint = { clientX: event.clientX, clientY: event.clientY, pointerType: event.pointerType };
  state.zoomPress = {
    pointerId: event.pointerId,
    pointerType: event.pointerType,
    startX: event.clientX,
    startY: event.clientY,
    timer: setTimeout(() => activateZoomFromHold(event.pointerId), ZOOM_LONG_PRESS_MS),
  };
  try {
    els.viewer.setPointerCapture(event.pointerId);
  } catch {
    // Some browsers reject capture for already-cancelled touch gestures.
  }
  return true;
}

function activateZoomFromHold(pointerId) {
  const press = state.zoomPress;
  if (!press || press.pointerId !== pointerId || !findImage(state.currentId)) return;
  state.zoomPress = null;
  state.zoomActive = true;
  state.zoomPointerId = pointerId;
  state.touchGesture = null;
  els.viewer.classList.add("zooming");
  els.zoomLoupe.hidden = false;
  const point = state.zoomLastPoint || { clientX: press.startX, clientY: press.startY };
  updateZoomLoupe(point.clientX, point.clientY, point.pointerType || press.pointerType);
}

function cancelZoomHold() {
  if (!state.zoomPress) return;
  clearTimeout(state.zoomPress.timer);
  state.zoomPress = null;
}

function stopZoom() {
  cancelZoomHold();
  state.zoomActive = false;
  state.zoomPointerId = null;
  state.zoomLastPoint = null;
  state.zoomFullActive = false;
  state.zoomFullLastPoint = null;
  clearZoomSource();
  els.viewer.classList.remove("zooming");
  els.viewer.classList.remove("zoom-full-active");
  els.zoomFull.hidden = true;
  els.zoomFull.style.removeProperty("background-image");
  els.zoomFull.style.removeProperty("background-size");
  els.zoomFull.style.removeProperty("background-position");
  els.zoomFull.style.removeProperty("filter");
  els.zoomLoupe.hidden = true;
  els.zoomLoupe.style.removeProperty("background-image");
  els.zoomLoupe.style.removeProperty("background-size");
  els.zoomLoupe.style.removeProperty("background-position");
  els.zoomLoupe.style.removeProperty("filter");
}

function clearZoomSource() {
  if (state.zoomSourceImage) {
    state.zoomSourceImage.onload = null;
    state.zoomSourceImage.onerror = null;
  }
  state.zoomSourceImage = null;
  state.zoomSourceUrl = null;
}

function updateZoomHold(event) {
  if (!state.zoomPress || state.zoomPress.pointerId !== event.pointerId) return;
  state.zoomLastPoint = { clientX: event.clientX, clientY: event.clientY, pointerType: event.pointerType };
  const dx = event.clientX - state.zoomPress.startX;
  const dy = event.clientY - state.zoomPress.startY;
  if (Math.hypot(dx, dy) > ZOOM_MOVE_CANCEL_PX) cancelZoomHold();
}

function updateZoomLoupe(clientX, clientY, pointerType = state.zoomLastPoint?.pointerType || "mouse") {
  const imageRect = els.image.getBoundingClientRect();
  const viewerRect = els.viewer.getBoundingClientRect();
  if (imageRect.width <= 1 || imageRect.height <= 1 || viewerRect.width <= 1 || viewerRect.height <= 1) return;

  state.zoomLastPoint = { clientX, clientY, pointerType };
  const loupeWidth = els.zoomLoupe.offsetWidth || 180;
  const loupeHeight = els.zoomLoupe.offsetHeight || loupeWidth;
  const { left, top } = zoomLoupePosition(clientX, clientY, viewerRect, loupeWidth, loupeHeight, pointerType);
  const relX = clamp((clientX - imageRect.left) / imageRect.width, 0, 1);
  const relY = clamp((clientY - imageRect.top) / imageRect.height, 0, 1);
  const zoomSource = zoomImageSource();
  const sourceWidth = zoomSource.width;
  const sourceHeight = zoomSource.height;
  const bgX = loupeWidth / 2 - relX * sourceWidth;
  const bgY = loupeHeight / 2 - relY * sourceHeight;
  const imageStyle = window.getComputedStyle(els.image);

  els.zoomLoupe.style.left = `${left}px`;
  els.zoomLoupe.style.top = `${top}px`;
  els.zoomLoupe.style.backgroundImage = `url("${cssUrl(zoomSource.url)}")`;
  els.zoomLoupe.style.backgroundSize = `${sourceWidth}px ${sourceHeight}px`;
  els.zoomLoupe.style.backgroundPosition = `${bgX}px ${bgY}px`;
  els.zoomLoupe.style.filter = imageStyle.filter === "none" ? "" : imageStyle.filter;
}

function toggleFullImageZoom(event) {
  if (!canToggleFullImageZoom(event)) return;
  event.preventDefault();
  if (state.zoomFullActive) {
    stopZoom();
    return;
  }

  stopZoom();
  state.zoomFullActive = true;
  state.zoomFullLastPoint = { clientX: event.clientX, clientY: event.clientY };
  els.viewer.classList.add("zoom-full-active");
  els.zoomFull.hidden = false;
  updateFullImageZoom(event.clientX, event.clientY);
}

function updateFullImageZoom(clientX, clientY) {
  if (!state.zoomFullActive) return;
  const imageRect = els.image.getBoundingClientRect();
  const frameRect = els.zoomFull.getBoundingClientRect();
  if (imageRect.width <= 1 || imageRect.height <= 1 || frameRect.width <= 1 || frameRect.height <= 1) return;

  state.zoomFullLastPoint = { clientX, clientY };
  const zoomSource = zoomImageSource();
  if (!zoomSource.url || zoomSource.width <= 0 || zoomSource.height <= 0) return;
  const minimumScale = Math.max((imageRect.width * 2) / zoomSource.width, (imageRect.height * 2) / zoomSource.height);
  const coverScale = Math.max(frameRect.width / zoomSource.width, frameRect.height / zoomSource.height);
  const scale = Math.max(1, minimumScale, coverScale);
  const zoomWidth = zoomSource.width * scale;
  const zoomHeight = zoomSource.height * scale;
  const relativeX = clamp((clientX - imageRect.left) / imageRect.width, 0, 1);
  const relativeY = clamp((clientY - imageRect.top) / imageRect.height, 0, 1);
  const pointerX = clamp(clientX - frameRect.left, 0, frameRect.width);
  const pointerY = clamp(clientY - frameRect.top, 0, frameRect.height);
  const backgroundX = fullZoomOffset(pointerX, relativeX, zoomWidth, frameRect.width);
  const backgroundY = fullZoomOffset(pointerY, relativeY, zoomHeight, frameRect.height);
  const imageStyle = window.getComputedStyle(els.image);

  els.zoomFull.style.backgroundImage = `url("${cssUrl(zoomSource.url)}")`;
  els.zoomFull.style.backgroundSize = `${zoomWidth}px ${zoomHeight}px`;
  els.zoomFull.style.backgroundPosition = `${backgroundX}px ${backgroundY}px`;
  els.zoomFull.style.filter = imageStyle.filter === "none" ? "" : imageStyle.filter;
}

function fullZoomOffset(pointer, relative, contentSize, frameSize) {
  if (contentSize <= frameSize) return (frameSize - contentSize) / 2;
  return clamp(pointer - relative * contentSize, frameSize - contentSize, 0);
}

function zoomImageSource() {
  const fallback = {
    url: els.image.currentSrc || els.image.src,
    width: els.image.naturalWidth || els.image.getBoundingClientRect().width,
    height: els.image.naturalHeight || els.image.getBoundingClientRect().height,
  };
  const image = findImage(state.currentId);
  if (!isDirectCompressedImage(image) || !image.full_url || compressedViewportUsesFullMedia()) return fallback;

  const fullUrl = versionedUrl(image.full_url, image.preview_updated_at || image.updated_at);
  if (state.zoomSourceUrl !== fullUrl) {
    clearZoomSource();
    const source = new Image();
    state.zoomSourceImage = source;
    state.zoomSourceUrl = fullUrl;
    source.onload = () => {
      if (state.zoomSourceImage !== source) return;
      if (state.zoomFullActive && state.zoomFullLastPoint) {
        updateFullImageZoom(state.zoomFullLastPoint.clientX, state.zoomFullLastPoint.clientY);
      } else if (state.zoomActive && state.zoomLastPoint) {
        updateZoomLoupe(state.zoomLastPoint.clientX, state.zoomLastPoint.clientY, state.zoomLastPoint.pointerType);
      }
    };
    source.src = fullUrl;
  }

  const source = state.zoomSourceImage;
  if (!source?.complete || source.naturalWidth <= 0 || source.naturalHeight <= 0) return fallback;
  return {
    url: state.zoomSourceUrl,
    width: source.naturalWidth,
    height: source.naturalHeight,
  };
}

function zoomLoupePosition(clientX, clientY, viewerRect, loupeWidth, loupeHeight, pointerType) {
  const pointerX = clientX - viewerRect.left;
  const pointerY = clientY - viewerRect.top;
  const gap = pointerType === "touch" ? ZOOM_LOUPE_TOUCH_GAP_PX : ZOOM_LOUPE_POINTER_GAP_PX;
  const maxLeft = Math.max(0, viewerRect.width - loupeWidth);
  const maxTop = Math.max(0, viewerRect.height - loupeHeight);
  const rightFits = pointerX + gap + loupeWidth <= viewerRect.width;
  const aboveFits = pointerY - gap - loupeHeight >= 0;
  const preferRight = rightFits || pointerX < viewerRect.width / 2;
  const preferAbove = aboveFits || pointerY >= viewerRect.height / 2;
  const left = preferRight ? pointerX + gap : pointerX - loupeWidth - gap;
  const top = preferAbove ? pointerY - loupeHeight - gap : pointerY + gap;

  return {
    left: clamp(left, 0, maxLeft),
    top: clamp(top, 0, maxTop),
  };
}

function cssUrl(value) {
  return String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

function startViewerTouch(event) {
  if (startZoomHold(event)) event.preventDefault();
  if (event.pointerType !== "touch" || state.cropEditing || !findImage(state.currentId)) return;
  const target = pointerTargetElement(event);
  if (target?.closest(".crop-overlay, .crop-tools")) return;
  state.touchGesture = { pointerId: event.pointerId, startX: event.clientX, startY: event.clientY };
  try {
    els.viewer.setPointerCapture(event.pointerId);
  } catch {
    // The browser may cancel touch capture during system gestures.
  }
}

function updateViewerTouch(event) {
  if (state.zoomFullActive && event.pointerType === "mouse") {
    updateFullImageZoom(event.clientX, event.clientY);
    return;
  }
  if (state.zoomActive && state.zoomPointerId === event.pointerId) {
    event.preventDefault();
    updateZoomLoupe(event.clientX, event.clientY, event.pointerType);
    return;
  }
  updateZoomHold(event);
  if (!state.touchGesture || state.touchGesture.pointerId !== event.pointerId) return;
  event.preventDefault();
}

async function endViewerTouch(event) {
  if (state.zoomActive && state.zoomPointerId === event.pointerId) {
    event.preventDefault();
    stopZoom();
    return;
  }
  if (state.zoomPress?.pointerId === event.pointerId) cancelZoomHold();
  const gesture = state.touchGesture;
  if (!gesture || gesture.pointerId !== event.pointerId) return;
  state.touchGesture = null;
  const dx = event.clientX - gesture.startX;
  const dy = event.clientY - gesture.startY;
  const absX = Math.abs(dx);
  const absY = Math.abs(dy);
  if (absX >= TOUCH_SWIPE_MIN_PX && absX / Math.max(1, absY) >= TOUCH_SWIPE_RATIO) {
    await move(dx > 0 ? -1 : 1);
    showCurrentRatingFeedback();
    return;
  }
  if (absY >= TOUCH_SWIPE_MIN_PX && absY / Math.max(1, absX) >= TOUCH_SWIPE_RATIO) {
    await rateCurrentWithoutAdvance(adjustedCurrentRating(dy < 0 ? 1 : -1));
  }
}

document.querySelectorAll(".rating button[data-rating]").forEach((button) => {
  button.addEventListener("click", () => rateCurrentAndAdvance(Number(button.dataset.rating)));
});

document.querySelectorAll(".labels button[data-label]").forEach((button) => {
  button.addEventListener("click", () => {
    toggleCurrentLabel(button.dataset.label);
  });
});

els.tags.addEventListener("change", () => saveReview());
els.tags.addEventListener("blur", () => saveReview());
els.tags.addEventListener("input", scheduleAutosave);
els.tags.addEventListener("keydown", confirmTagsInput);
els.notes.addEventListener("change", () => saveReview());
els.notes.addEventListener("blur", () => saveReview());
els.notes.addEventListener("input", scheduleAutosave);
els.notes.addEventListener("keydown", confirmMetadataInput);
els.image.addEventListener("load", () => {
  if (state.zoomActive) {
    stopZoom();
  } else if (state.zoomFullActive && state.zoomFullLastPoint) {
    clearZoomSource();
    updateFullImageZoom(state.zoomFullLastPoint.clientX, state.zoomFullLastPoint.clientY);
  }
  scheduleHistogramRender();
  scheduleViewerSafeAreaUpdate();
  renderRetouchGrid(findImage(state.currentId));
  renderFocusOverlay(findImage(state.currentId));
  renderCropOverlay(findImage(state.currentId));
});
els.cropSourceImage.addEventListener("load", initializeCropGeometry);
els.cropCurrentImage.addEventListener("load", () => layoutCropStage());
els.viewer.addEventListener("pointerdown", startViewerTouch);
els.viewer.addEventListener("pointermove", updateViewerTouch);
els.viewer.addEventListener("dblclick", toggleFullImageZoom);
els.viewer.addEventListener("pointerup", (event) => {
  endViewerTouch(event).catch((error) => console.error(error));
});
els.viewer.addEventListener("pointercancel", (event) => {
  if (state.zoomActive && state.zoomPointerId === event.pointerId) stopZoom();
  if (state.zoomPress?.pointerId === event.pointerId) cancelZoomHold();
  state.touchGesture = null;
});
els.viewer.addEventListener("contextmenu", (event) => {
  if (state.zoomActive || state.zoomFullActive || state.zoomPress || isViewerGestureSurface(event))
    event.preventDefault();
});
els.viewer.addEventListener("dragstart", preventNativeViewerAction);
els.viewer.addEventListener("selectstart", preventNativeViewerAction);
els.viewer.addEventListener("touchstart", preventNativeViewerAction, { passive: false });
els.viewer.addEventListener("touchmove", preventNativeViewerAction, { passive: false });
[
  els.retouchExposure,
  els.retouchContrast,
  els.retouchHighlights,
  els.retouchShadows,
  els.retouchWhites,
  els.retouchBlacks,
  els.retouchTemperature,
  els.retouchOffset,
  els.retouchClarity,
].forEach((input) => {
  input.addEventListener("input", () => {
    const retouch = retouchFromInputs();
    updateRetouchReadouts(retouch);
    applyLocalRetouch(retouch);
  });
  input.addEventListener("focus", () => setActiveRetouchSlider(input));
  input.addEventListener("click", () => setActiveRetouchSlider(input));
  input.addEventListener("blur", () => {
    if (state.retouchActiveSliderId === input.id) {
      clearActiveRetouchSlider();
    }
  });
  input.addEventListener("change", () => scheduleRetouchSave());
});
document.addEventListener("pointerdown", maybeClearRetouchSliderActivation);
els.retouchReset.addEventListener("click", () => applyLocalRetouch(defaultRetouch()));
els.retouchCopy.addEventListener("click", copyCurrentRetouch);
els.retouchPaste.addEventListener("click", pasteCurrentRetouch);
syncRetouchClipboardButtons();
els.cropReset.addEventListener("click", clearCropDraft);
els.cropToggle.addEventListener("click", beginCropEditing);
els.cropOk.addEventListener("click", approveCropEditing);
els.cropCancel.addEventListener("click", cancelCropEditing);
els.cropRotation.addEventListener("input", () => setCropDraftRotation(Number(els.cropRotation.value || 0)));
els.cropRotateLeft.addEventListener("click", () => setCropDraftRotation(state.cropDraftRotation - 90));
els.cropRotateRight.addEventListener("click", () => setCropDraftRotation(state.cropDraftRotation + 90));
els.cropRatio.addEventListener("change", () => setCropRatioPreset(els.cropRatio.value));
els.cropBox.addEventListener("pointerdown", startCropDrag);
els.cropBox.addEventListener("pointermove", updateCropDrag);
els.cropBox.addEventListener("pointerup", endCropDrag);
els.cropBox.addEventListener("pointercancel", endCropDrag);
document.querySelectorAll(".retouch label > span").forEach((label) => {
  label.title = "Double-click to reset";
  label.addEventListener("dblclick", (event) => {
    event.preventDefault();
    const image = findImage(state.currentId);
    if (isRetouchControlsDisabledForImage(image)) return;
    const input = label.parentElement?.querySelector('input[type="range"]');
    if (!input) return;
    input.value = input.defaultValue || "0";
    const retouch = retouchFromInputs();
    updateRetouchReadouts(retouch);
    applyLocalRetouch(retouch);
  });
});
els.minRating.addEventListener("change", () => {
  updateSharedUi({ current_image_id: state.currentId, min_rating: minRating() }).catch((error) => console.error(error));
});

let autosaveTimer = null;
let retouchSaveTimer = null;
function scheduleAutosave() {
  clearTimeout(autosaveTimer);
  autosaveTimer = setTimeout(() => saveCurrentIfNeeded(), 500);
}

function confirmMetadataInput(event) {
  if (blurMetadataInput(event)) return;
  if (event.key !== "Enter") return;
  event.preventDefault();
  clearTimeout(autosaveTimer);
  event.currentTarget.blur();
  saveCurrentIfNeeded().catch((error) => console.error(error));
}

function confirmTagsInput(event) {
  if (blurMetadataInput(event)) return;
  if (event.key !== "Enter") return;
  event.preventDefault();
  clearTimeout(autosaveTimer);
  event.currentTarget.blur();
  move(1).catch((error) => console.error(error));
}

function blurMetadataInput(event) {
  if (event.key !== "Escape") return false;
  event.preventDefault();
  clearTimeout(autosaveTimer);
  event.currentTarget.blur();
  return true;
}

function focusMetadataInput(input, { select = true } = {}) {
  const placeCursorToEnd = () => {
    const end = input.value.length;
    input.setSelectionRange(end, end);
  };

  if (isMobileReviewLayout()) {
    setMobileDrawer("metadata");
    requestAnimationFrame(() => {
      input.focus();
      if (select) {
        input.select();
      } else {
        placeCursorToEnd();
      }
    });
    return;
  }
  input.focus();
  if (select) {
    input.select();
    return;
  }
  placeCursorToEnd();
}

function scheduleRetouchSave() {
  clearRetouchSaveTimer();
  retouchSaveTimer = setTimeout(() => {
    retouchSaveTimer = null;
    saveReview({ retouch: retouchFromInputs() }).catch((error) => console.error(error));
  }, RETOUCH_SAVE_DEBOUNCE_MS);
}

function clearRetouchSaveTimer() {
  clearTimeout(retouchSaveTimer);
  retouchSaveTimer = null;
}

els.publish.addEventListener("click", () => togglePublishWizard(true));
els.mobilePublish.addEventListener("click", () => togglePublishWizard(true));
els.diffusion.addEventListener("click", openDiffusion);
els.diffusionOverlay.addEventListener("click", (event) => {
  if (event.target === els.diffusionOverlay) closeDiffusion();
});
els.sampler.addEventListener("click", () => openSampler().catch((error) => console.error(error)));
els.samplerOverlay.addEventListener("click", (event) => {
  if (event.target === els.samplerOverlay) closeSampler();
});
els.panorama.addEventListener("click", openPanoramaWizard);
els.panoramaOverlay.addEventListener("click", (event) => {
  if (event.target === els.panoramaOverlay) closePanoramaWizard();
});
els.mobileSaveOriginal.addEventListener("click", () => {
  saveOriginalPhoto().catch((error) => console.error(error));
});
els.appVersion?.addEventListener("click", (event) => {
  event.preventDefault();
  openCommandInvocation();
});
els.mobileDrawerButtons.forEach((button) => {
  button.addEventListener("click", () => toggleMobileDrawer(button.dataset.mobileDrawer));
});
els.publishCancel.addEventListener("click", () => togglePublishWizard(false));
els.publishOverlay.addEventListener("click", (event) => {
  if (event.target === els.publishOverlay) togglePublishWizard(false);
});
els.profileInfoOverlay?.addEventListener("click", (event) => {
  if (event.target === els.profileInfoOverlay) {
    closeProfileInfo();
  }
});
els.commandInvocationOverlay?.addEventListener("click", (event) => {
  if (event.target === els.commandInvocationOverlay) {
    closeCommandInvocation();
  }
});
els.publishForm.addEventListener("input", updatePublishModeText);
els.publishForm.addEventListener("change", (event) => {
  if (event.target === els.publishSizeMode) {
    syncPublishSizeFields();
  }
  if (event.target === els.publishNormalizeGrain) {
    syncPublishNormalizeGrainField();
  }
  updatePublishModeText();
});
els.publishForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  els.publishSubmit.disabled = true;
  let started = false;
  try {
    const response = await fetch(reviewUrl("api/publish"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(publishFormBody()),
    });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || `publish ${response.status}`);
    started = true;
    applyStateMessage(data);
  } catch (error) {
    els.publishStatus.textContent = `Publish failed: ${error.message}`;
  } finally {
    if (started) {
      updatePublishStatus();
    } else {
      els.publishSubmit.disabled = false;
    }
  }
});

els.shortcutsHelp.addEventListener("click", () => toggleShortcuts(true));
els.shortcutsClose.addEventListener("click", () => toggleShortcuts(false));
els.shortcutsOverlay.addEventListener("click", (event) => {
  if (event.target === els.shortcutsOverlay) toggleShortcuts(false);
});

window.addEventListener("keydown", (event) => {
  const activeRetouchSliderElement = activeRetouchSlider();
  if (activeRetouchSliderElement && event.key === "Escape") {
    event.preventDefault();
    revertActiveRetouchSlider();
    return;
  }
  if (state.profileInfoProfileIndex !== null && event.key === "Escape") {
    event.preventDefault();
    closeProfileInfo();
    return;
  }
  if (state.commandInvocationOpen && event.key === "Escape") {
    event.preventDefault();
    closeCommandInvocation();
    return;
  }
  if (state.diffusionOpen) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeDiffusion();
    }
    return;
  }
  if (state.samplerOpen) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeSampler();
    }
    return;
  }
  if (state.panoramaOpen) {
    if (event.key === "Escape") {
      event.preventDefault();
      closePanoramaWizard();
    }
    return;
  }
  if (!els.publishOverlay.hidden) {
    if (event.key === "Escape") {
      event.preventDefault();
      togglePublishWizard(false);
    }
    if (event.target.closest(".publish-card")) return;
  }
  if (!els.shortcutsOverlay.hidden) {
    if (event.key === "Escape" || event.key === "?" || (event.key === "/" && event.shiftKey)) {
      event.preventDefault();
      toggleShortcuts(false);
    }
    return;
  }
  if (event.target === els.tags) return;
  if (event.target === els.notes) return;
  if (event.target === els.minRating) return;
  if (event.key === "Escape" && state.zoomFullActive) {
    event.preventDefault();
    stopZoom();
    return;
  }
  if (event.key === "Escape" && state.histogramOpen) {
    event.preventDefault();
    toggleHistogram(false);
    return;
  }
  if (event.key === "Escape" && state.informationOpen) {
    event.preventDefault();
    toggleInformation(false);
    return;
  }
  if (event.key === "Escape" && state.mobileDrawer) {
    event.preventDefault();
    setMobileDrawer(null);
    return;
  }
  if (event.key === "?" || (event.key === "/" && event.shiftKey)) {
    event.preventDefault();
    toggleShortcuts(true);
    return;
  }
  if (event.key === ",") {
    event.preventDefault();
    focusMetadataInput(els.tags, { select: false });
    return;
  }
  if (event.key === "/") {
    event.preventDefault();
    focusMetadataInput(els.notes);
    return;
  }
  const shortcutKey = event.key.toLowerCase();
  const plainShortcut = !event.ctrlKey && !event.metaKey && !event.altKey;
  if (state.cropEditing && plainShortcut && shortcutKey === "r") {
    event.preventDefault();
    rotateCropRatio();
    return;
  }
  if (plainShortcut && shortcutKey === "c") {
    const image = findImage(state.currentId);
    if (isRetouchControlsDisabledForImage(image)) return;
    event.preventDefault();
    copyCurrentRetouch();
    return;
  }
  if (plainShortcut && shortcutKey === "v") {
    const image = findImage(state.currentId);
    if (isRetouchControlsDisabledForImage(image)) return;
    event.preventDefault();
    pasteCurrentRetouch();
    return;
  }
  if (
    activeRetouchSliderElement &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
  ) {
    event.preventDefault();
    const direction = ["ArrowLeft", "ArrowDown"].includes(event.key) ? -1 : 1;
    nudgeRetouchSlider(activeRetouchSliderElement, direction, event.shiftKey);
    return;
  }
  if (activeRetouchSliderElement && event.key === "Enter") {
    event.preventDefault();
    commitActiveRetouchSlider();
    return;
  }
  if (event.target.closest(".retouch") || event.target.closest(".crop-tools")) return;
  if (plainShortcut && shortcutKey === "h") {
    event.preventDefault();
    toggleHistogram();
    return;
  }
  if (plainShortcut && shortcutKey === "i") {
    event.preventDefault();
    toggleInformation();
    return;
  }
  if (event.key === "ArrowRight" || event.key === "Enter") move(1);
  if (event.key === "ArrowLeft") move(-1);
  if (event.key === "PageDown") {
    event.preventDefault();
    selectProfileRelative(1);
  }
  if (event.key === "PageUp") {
    event.preventDefault();
    selectProfileRelative(-1);
  }
  if (event.key === " ") {
    event.preventDefault();
    toggleSelectedProfileAvailability();
  }
  if (event.key === "ArrowUp") {
    event.preventDefault();
    rateCurrentAndAdvance(adjustedCurrentRating(1));
  }
  if (event.key === "ArrowDown") {
    event.preventDefault();
    rateCurrentAndAdvance(adjustedCurrentRating(-1));
  }
  if (event.key.toLowerCase() === "f") {
    event.preventDefault();
    toggleFullscreen().catch((error) => console.error(error));
  }
  if (event.key === "`" || event.key === "§") {
    event.preventDefault();
    rateCurrentAndAdvance(0);
  }
  if (["1", "2", "3", "4", "5"].includes(event.key)) {
    event.preventDefault();
    rateCurrentAndAdvance(Number(event.key));
  }
  if (["6", "7", "8", "9", "0"].includes(event.key)) {
    event.preventDefault();
    const label = { 6: "red", 7: "yellow", 8: "green", 9: "blue", 0: "purple" }[event.key];
    toggleCurrentLabel(label);
  }
  if (["r", "y", "g", "b", "p", "n"].includes(event.key.toLowerCase())) {
    event.preventDefault();
    const label = { r: "red", y: "yellow", g: "green", b: "blue", p: "purple", n: "none" }[event.key.toLowerCase()];
    toggleCurrentLabel(label);
  }
});

els.workspace.addEventListener("wheel", handleNavigationWheel, { passive: false });

window.addEventListener("beforeunload", () => {
  const image = findImage(state.currentId);
  if (!image || !navigator.sendBeacon) return;
  const body = JSON.stringify(reviewRequestBody(image, {}, { useInputs: true }));
  navigator.sendBeacon(reviewUrl("api/review"), new Blob([body], { type: "application/json" }));
});

wideProfilesQuery.addEventListener("change", () => {
  syncProfilesPlacement();
  scheduleViewerSafeAreaUpdate();
});
mobileReviewQuery.addEventListener("change", syncMobileReviewLayout);
window.addEventListener("resize", () => {
  syncMainImageForViewport();
  scheduleViewerSafeAreaUpdate();
  scheduleHistogramRender({ debounce: true });
});
document.addEventListener("fullscreenchange", () => {
  syncMainImageForViewport();
  scheduleViewerSafeAreaUpdate();
});

if ("ResizeObserver" in window) {
  state.viewerSafeAreaObserver = new ResizeObserver(scheduleViewerSafeAreaUpdate);
  state.viewerSafeAreaObserver.observe(els.workspace);
  state.viewerSafeAreaObserver.observe(els.panel);
  state.viewerSafeAreaObserver.observe(els.profiles);
}

function connectEvents() {
  const events = new EventSource(reviewUrl("api/events"));
  events.onopen = () => {
    els.liveDot.classList.add("connected");
  };
  events.onmessage = (event) => {
    els.liveDot.classList.add("connected");
    applyStateMessage(JSON.parse(event.data));
  };
  events.addEventListener("keepalive", (event) => {
    els.liveDot.classList.add("connected");
    els.liveDot.classList.remove("keepalive-pulse");
    void els.liveDot.offsetWidth;
    els.liveDot.classList.add("keepalive-pulse");
    try {
      const data = JSON.parse(event.data);
      els.liveDot.title = `Connected · ${data.datetime || "keepalive"} · mini-film ${data.version || ""}`.trim();
    } catch {
      els.liveDot.title = "Connected";
    }
  });
  events.onerror = () => {
    els.liveDot.classList.remove("connected", "keepalive-pulse");
    els.liveDot.title = "Reconnecting";
    els.status.textContent = "Reconnecting...";
  };
}

loadState()
  .then(connectEvents)
  .catch((error) => {
    els.status.textContent = `Disconnected: ${error.message}`;
    setTimeout(() => window.location.reload(), 1500);
  });

const tuckedProfileRailQuery = window.matchMedia(
  "(min-width: 901px) and (min-height: 620px) and (max-width: 1499.98px)",
);

function setProfileRailOpen(open) {
  if (!els.profiles || !tuckedProfileRailQuery.matches) return;
  els.profiles.classList.toggle("peek-open", open);
}

els.profiles?.addEventListener("pointerdown", (event) => {
  if (!tuckedProfileRailQuery.matches || event.pointerType === "mouse") return;
  if (els.profiles.classList.contains("peek-open")) return;
  setProfileRailOpen(true);
  event.preventDefault();
  event.stopPropagation();
});

els.image?.addEventListener("pointerdown", () => setProfileRailOpen(false));

tuckedProfileRailQuery.addEventListener("change", () => els.profiles?.classList.remove("peek-open"));
