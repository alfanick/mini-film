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
  cropDrag: null,
  cropPointers: new Map(),
  cropTouchGesture: null,
  touchGesture: null,
  zoomPress: null,
  zoomActive: false,
  zoomPointerId: null,
  zoomLastPoint: null,
  gestureFeedbackTimer: null,
  retouchInputImageId: null,
  localRetouchDirty: false,
  mobileDrawer: null,
};

const RETOUCH_SAVE_DEBOUNCE_MS = 1200;
const TOUCH_SWIPE_MIN_PX = 72;
const TOUCH_SWIPE_RATIO = 1.65;
const ZOOM_LONG_PRESS_MS = 380;
const ZOOM_MOVE_CANCEL_PX = 22;
const ZOOM_SCALE = 2.6;
const WHEEL_NAV_THRESHOLD_PX = 90;
const WHEEL_NAV_RESET_MS = 220;
const WHEEL_NAV_COOLDOWN_MS = 260;
const RATING_VALUES = [0, 1, 2, 3, 4, 5];
const COLOR_LABELS = ["red", "yellow", "green", "blue", "purple"];

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
      ),
      h(
        "main",
        { class: "workspace" },
        h(
          "section",
          { class: "viewer" },
          h("div", { id: "empty", class: "empty" }, "Waiting for pictures"),
          h("img", { id: "main-image", alt: "", draggable: false }),
          h("div", { id: "gesture-feedback", class: "gesture-feedback", hidden: true }),
          h("div", { id: "zoom-loupe", class: "zoom-loupe", hidden: true }),
          h("div", { id: "retouch-grid", class: "retouch-grid", hidden: true }),
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
            h(
              "div",
              { id: "crop-tools", class: "crop-tools", hidden: true },
              h("button", { id: "crop-rotate-left", type: "button" }, "-90"),
              h(
                "label",
                null,
                h("span", null, "Rotate"),
                h("input", { id: "crop-rotation", type: "range", min: "-180", max: "180", step: "0.25", value: "0" }),
                h("output", { id: "crop-rotation-value" }, "0"),
              ),
              h("button", { id: "crop-rotate-right", type: "button" }, "+90"),
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
              h("div", { id: "image-subtitle", class: "image-subtitle" }),
            ),
            h("div", { id: "profile-state", class: "profile-state" }),
          ),
          h(
            "div",
            { class: "mobile-actions", "aria-label": "Review tools" },
            h("button", { "data-mobile-drawer": "profiles", type: "button" }, "Profiles"),
            h("button", { "data-mobile-drawer": "retouch", type: "button" }, "Retouch"),
            h("button", { "data-mobile-drawer": "metadata", type: "button" }, "Meta"),
            h("button", { id: "mobile-publish", type: "button" }, "Publish"),
          ),
          h("div", { id: "profiles", class: "profiles" }),
          h(ControlsShell),
        ),
      ),
    ),
    h(ShortcutsOverlay),
    h(PublishOverlay),
  );
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
        h("button", { id: "retouch-reset", type: "button" }, "Reset"),
      ),
      h(RetouchSlider, { id: "retouch-exposure", label: "Exposure", min: "-4", max: "4", step: "0.05", value: "0" }),
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
        label: "Temp",
        min: "-2500",
        max: "2500",
        step: "50",
        value: "0",
        output: "0K",
      }),
      h(
        "div",
        { class: "retouch-actions" },
        h("button", { id: "crop-toggle", type: "button" }, "Crop/rotate"),
        h("button", { id: "crop-ok", type: "button", hidden: true }, "OK"),
        h("button", { id: "crop-cancel", type: "button", hidden: true }, "Cancel"),
        h("button", { id: "crop-reset", type: "button" }, "Clear"),
      ),
      h(RetouchSlider, { id: "retouch-clarity", label: "Contrast", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, { id: "retouch-shadows", label: "Shadows", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, { id: "retouch-blacks", label: "Blacks", min: "-100", max: "100", step: "1", value: "0" }),
      h(RetouchSlider, { id: "retouch-offset", label: "Offset", min: "-100", max: "100", step: "1", value: "0" }),
    ),
  );
}

function RetouchSlider({ id, label, min, max, step, value, output = value }) {
  return h(
    "label",
    null,
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
        [["h", "l", "Enter"], "Alternative navigation keys for the same previous or next action."],
      ],
    ],
    [
      "Touch / Mouse",
      [
        [["Swipe ←/→"], "Move between visible pictures without changing the rating."],
        [["Swipe ↑/↓"], "Change the rating without advancing."],
        [["Wheel ←/→"], "Move between visible pictures after a short scroll threshold."],
        [["Wheel ↑/↓"], "Preview the previous or next profile after a short scroll threshold."],
        [["Mouse Back/Forward"], "Decrease or increase the rating without advancing."],
        [["Hold"], "Zoom into the picture under the cursor or finger until released."],
        [["Profile"], "Click a profile thumbnail to preview it; use its checkbox to include it in publishing."],
      ],
    ],
    [
      "Rating",
      [
        [["1", "2", "3", "4", "5"], "Set rating and advance to the next visible picture."],
        [["↑", "↓"], "Increase or decrease the rating, then advance."],
      ],
    ],
    [
      "Labels",
      [
        [["6", "7", "8", "9", "0"], "Toggle red, yellow, green, blue, or purple labels without advancing."],
        [["r", "y", "g", "b", "v"], "Same label toggles using mnemonic keys."],
        [["n"], "Clear all color labels."],
      ],
    ],
    [
      "Profiles",
      [
        [["PgUp", "PgDn"], "Preview the previous or next profile for the current picture."],
        [["Space"], "Include or skip the selected profile when publishing."],
      ],
    ],
    [
      "Metadata",
      [
        [[","], "Focus tags."],
        [["/"], "Focus notes."],
        [["Enter"], "Save tags and advance; save notes and return to review."],
      ],
    ],
    [
      "View",
      [
        [["f"], "Toggle fullscreen."],
        [["?", "Esc"], "Show or hide this shortcuts overlay."],
      ],
    ],
    [
      "Retouch",
      [
        [["Double-click"], "Double-click a retouch control name to reset that value."],
        [["Crop", "OK"], "Open crop/rotate, adjust the frame, then apply it with OK."],
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
  gestureFeedback: document.getElementById("gesture-feedback"),
  zoomLoupe: document.getElementById("zoom-loupe"),
  title: document.getElementById("image-title"),
  subtitle: document.getElementById("image-subtitle"),
  profileState: document.getElementById("profile-state"),
  profiles: document.getElementById("profiles"),
  controls: document.querySelector(".controls"),
  imageExif: document.getElementById("image-exif"),
  tags: document.getElementById("tags"),
  notes: document.getElementById("notes"),
  retouchGrid: document.getElementById("retouch-grid"),
  cropOverlay: document.getElementById("crop-overlay"),
  cropBox: document.getElementById("crop-box"),
  cropTools: document.getElementById("crop-tools"),
  cropRotation: document.getElementById("crop-rotation"),
  cropRotationValue: document.getElementById("crop-rotation-value"),
  cropRotateLeft: document.getElementById("crop-rotate-left"),
  cropRotateRight: document.getElementById("crop-rotate-right"),
  retouchReset: document.getElementById("retouch-reset"),
  retouchExposure: document.getElementById("retouch-exposure"),
  retouchExposureValue: document.getElementById("retouch-exposure-value"),
  retouchHighlights: document.getElementById("retouch-highlights"),
  retouchHighlightsValue: document.getElementById("retouch-highlights-value"),
  retouchShadows: document.getElementById("retouch-shadows"),
  retouchShadowsValue: document.getElementById("retouch-shadows-value"),
  retouchWhites: document.getElementById("retouch-whites"),
  retouchWhitesValue: document.getElementById("retouch-whites-value"),
  retouchBlacks: document.getElementById("retouch-blacks"),
  retouchBlacksValue: document.getElementById("retouch-blacks-value"),
  retouchTemperature: document.getElementById("retouch-temperature"),
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
  minRating: document.getElementById("min-rating"),
  app: document.querySelector(".app"),
  shortcutsHelp: document.getElementById("shortcuts-help"),
  mobileDrawerButtons: document.querySelectorAll("[data-mobile-drawer]"),
  mobilePublish: document.getElementById("mobile-publish"),
  shortcutsOverlay: document.getElementById("shortcuts-overlay"),
  shortcutsClose: document.getElementById("shortcuts-close"),
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

function applyState(data) {
  if (state.data?.version && data?.version && state.data.version !== data.version) {
    window.location.reload();
    return;
  }
  state.data = data;
  state.localRetouchDirty = false;
  applyServerUi(data);
  render();
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
  syncProfilesPlacement();
  const images = filteredImages();
  const total = state.data?.images?.length || 0;
  const profileCount = state.data?.profiles?.length || 0;
  const clientCount = state.data?.client_count || 0;
  const publishSummary = latestPublishJobSummary();
  els.appVersion.textContent = `mini-film ${state.data?.version || ""}`.trim();
  els.status.textContent = `${images.length}/${total} pictures | ${profileCount} ${plural(profileCount, "profile")} | ${clientCount} ${plural(clientCount, "client")}${publishSummary ? ` | ${publishSummary}` : ""}`;
  updatePublishStatus();
  renderList(images);
  let current = findImage(state.currentId);
  if (current && !passesFilter(current)) current = null;
  if (!current) {
    state.currentId = firstReviewableImageId();
    current = findImage(state.currentId);
  }
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
    return `Published ${job.linked} files to ${job.album}; skipped ${job.skipped}; galleries ${job.galleries}.`;
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
  const profileSafe =
    els.profiles.parentElement === els.workspace
      ? Math.max(0, Math.ceil(workspaceRect.right - els.profiles.getBoundingClientRect().left))
      : 0;

  els.workspace.style.setProperty("--review-panel-safe", `${panelSafe}px`);
  els.workspace.style.setProperty("--review-profile-safe", `${profileSafe}px`);
  if (!els.retouchGrid.hidden) positionRetouchGrid();
  if (!els.cropOverlay.hidden) positionCropOverlay();
  if (!els.gestureFeedback.hidden) positionGestureFeedback();
  if (state.zoomActive && state.zoomLastPoint)
    updateZoomLoupe(state.zoomLastPoint.clientX, state.zoomLastPoint.clientY);
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
  preactRender(
    h(ImageList, {
      images,
      currentId: state.currentId,
      onSelect: async (image) => {
        const carryProfileIndex = selectedProfile(findImage(state.currentId))?.profile_index;
        await saveCurrentIfNeeded();
        await updateSharedUi({ current_image_id: image.id, min_rating: minRating() });
        await carrySelectedProfileToImage(image.id, carryProfileIndex);
      },
    }),
    els.list,
  );
  const activeRow = els.list.querySelector(".image-row.active");
  if (activeRow) {
    requestAnimationFrame(() => {
      activeRow.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
  }
}

function ImageList({ images, currentId, onSelect }) {
  return images.map((image) => {
    const progress = renderProgressSummary(image);
    const labels = imageLabels(image);
    const isActive = image.id === currentId;
    return h(
      "button",
      {
        key: image.id,
        type: "button",
        class: `image-row${isActive ? " active" : ""}`,
        onClick: () => onSelect(image).catch((error) => console.error(error)),
      },
      h("img", {
        class: "image-row-thumb",
        alt: "",
        src: image.preview_url || undefined,
      }),
      h(
        "div",
        {
          class: "image-row-title",
          title: image.relative_path || image.file_name,
        },
        image.file_name,
      ),
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
  });
}

function renderProgressSummary(image) {
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
  if (total === 0) {
    return {
      state: "waiting",
      text: "none",
      title: "no profiles selected for publish",
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

function renderCurrent(image) {
  if (!image) {
    stopZoom();
    clearCropDraftState();
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
    els.title.textContent = "";
    els.subtitle.textContent = "";
    els.profileState.textContent = "";
    els.imageExif.replaceChildren();
    preactRender(null, els.profiles);
    els.tags.value = "";
    els.notes.value = "";
    state.lastInputImageId = null;
    setActiveReviewButtons(null);
    updateMobileActionLabels(null);
    return;
  }

  const selected = selectedProfile(image);
  const mainUrl = selected?.url || image.preview_url;
  const previewNote = selected?.url ? "" : image.preview_url ? " | camera preview" : "";
  const selectedState = profileDisplayState(image, selected);
  if (state.cropDraftImageId !== null && state.cropDraftImageId !== image.id) {
    clearCropDraftState();
  }
  els.title.textContent = image.file_name;
  els.subtitle.textContent = `${image.relative_path} | rating ${image.rating}`;
  renderImageExif(image);
  els.profileState.textContent = selected ? `${selected.profile_stem}: ${selectedState.text}${previewNote}` : "";
  const imageChanged = state.lastInputImageId !== image.id;
  if (imageChanged || document.activeElement !== els.tags) {
    els.tags.value = image.tags.join(", ");
  }
  if (imageChanged || document.activeElement !== els.notes) {
    els.notes.value = image.notes || "";
  }
  if (imageChanged || !isRetouchControlActive()) {
    setRetouchInputs(image.retouch || defaultRetouch());
  }
  state.lastInputImageId = image.id;
  setActiveReviewButtons(image);

  if (mainUrl) {
    els.viewer.classList.add("has-image");
    const stamp = selected?.url ? selected.updated_at : image.preview_updated_at;
    const nextSrc = versionedUrl(mainUrl, stamp);
    if (els.image.getAttribute("src") !== nextSrc) stopZoom();
    els.image.src = nextSrc;
    els.image.alt = image.file_name;
  } else {
    stopZoom();
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
  }

  applyDraftRetouch(image, selected);
  renderRetouchGrid(image, selected);
  renderCropOverlay(image);
  renderProfiles(image);
  updateMobileActionLabels(image);
  preloadNearbyImages(image);
}

function renderImageExif(image) {
  els.imageExif.replaceChildren();
  const exif = image?.exif || {};
  const parts = [
    exif.shooting_mode ? `Mode ${exif.shooting_mode}` : "",
    exif.camera_model || "",
    exif.focal_length || "",
    exif.iso ? `ISO ${exif.iso}` : "",
    exif.aperture || "",
    exif.shutter_speed || "",
  ].filter(Boolean);
  const text = parts.join(" · ");
  els.imageExif.textContent = text;
  els.imageExif.title = text;
}

function selectedProfile(image) {
  return selectedProfileForImage(image);
}

function selectedProfileForImage(image) {
  const profiles = image?.profiles || [];
  const selected = profiles.find((profile) => profile.profile_index === image.selected_profile_index);
  const fallback = selected || profiles[0] || null;
  const publishIndexes = new Set(publishProfileIndexes(image));
  if (fallback && publishIndexes.size > 0 && !publishIndexes.has(fallback.profile_index)) {
    return profiles.find((profile) => publishIndexes.has(profile.profile_index)) || fallback;
  }
  return fallback;
}

function isLocalRetouchDraft(image) {
  return Boolean(image && image.id === state.currentId && state.localRetouchDirty);
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
  if (Array.isArray(image.publish_profile_indexes)) return image.publish_profile_indexes;
  return (image.profiles || []).map((profile) => profile.profile_index);
}

function togglePublishProfile(image, profileIndex) {
  const selected = new Set(publishProfileIndexes(image));
  if (selected.has(profileIndex)) {
    selected.delete(profileIndex);
  } else {
    selected.add(profileIndex);
  }
  return (image.profiles || [])
    .map((profile) => profile.profile_index)
    .filter((profileIndex) => selected.has(profileIndex));
}

function renderProfiles(image) {
  preactRender(
    h(ProfileList, {
      image,
      onSelect: async (profile) => {
        await saveReview({ selected_profile_index: profile.profile_index });
      },
      onTogglePublish: async (profile) => {
        await saveReview({ publish_profile_indexes: togglePublishProfile(image, profile.profile_index) });
      },
    }),
    els.profiles,
  );
}

function ProfileList({ image, onSelect, onTogglePublish }) {
  if (!image) return null;
  const publishIndexes = new Set(publishProfileIndexes(image));
  const previewProfile = selectedProfile(image);
  return (image.profiles || []).map((profile) => {
    const cardUrl = profile.url || image.preview_url;
    const publishSelected = publishIndexes.has(profile.profile_index);
    const display = profileDisplayState(image, profile);
    const sourceStatus = profile.url ? display.text : `${display.text} | preview`;
    const classes = [
      "profile-card",
      profile.profile_index === previewProfile?.profile_index ? "active" : "",
      profile.url ? "" : "pending",
      display.state,
      publishSelected ? "publish-selected" : "publish-excluded",
    ]
      .filter(Boolean)
      .join(" ");
    return h(
      "button",
      {
        key: profile.profile_index,
        type: "button",
        class: classes,
        onClick: () => onSelect(profile).catch((error) => console.error(error)),
      },
      h("input", {
        type: "checkbox",
        class: "profile-publish",
        checked: publishSelected,
        title: publishSelected ? "Included in publish" : "Skipped by publish",
        "aria-label": `Publish ${profile.profile_stem}`,
        onClick: (event) => event.stopPropagation(),
        onChange: (event) => {
          event.stopPropagation();
          onTogglePublish(profile).catch((error) => console.error(error));
        },
      }),
      cardUrl
        ? h("img", {
            src: versionedUrl(cardUrl, profile.url ? profile.updated_at : image.preview_updated_at),
            alt: profile.profile_stem,
            onLoad: (event) => {
              event.currentTarget
                .closest(".profile-card")
                ?.classList.toggle("portrait", event.currentTarget.naturalHeight > event.currentTarget.naturalWidth);
            },
          })
        : null,
      h("div", { class: "profile-name" }, profile.profile_stem),
      h(
        "div",
        {
          class: "profile-status",
          title: display.title,
        },
        `${sourceStatus} | ${publishSelected ? "publish" : "skip"}`,
      ),
    );
  });
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
  const profileCount = image?.profiles?.length || 0;
  const tagsCount = image?.tags?.length || 0;
  const hasNotes = Boolean(image?.notes);
  const retouchActive = image ? !retouchIsDefault(image.retouch || defaultRetouch()) : false;
  els.mobileDrawerButtons.forEach((button) => {
    const drawer = button.dataset.mobileDrawer;
    if (drawer === "profiles") {
      button.textContent = profileCount > 0 ? `Profiles ${profileCount}` : "Profiles";
      button.title = `${profileCount} profile ${profileCount === 1 ? "render" : "renders"}`;
    } else if (drawer === "retouch") {
      button.textContent = retouchActive ? "Retouch *" : "Retouch";
      button.title = retouchActive ? "Retouch adjustments are active" : "Retouch";
    } else if (drawer === "metadata") {
      button.textContent = tagsCount > 0 || hasNotes ? "Meta *" : "Meta";
      button.title = `${tagsCount} ${plural(tagsCount, "tag")}${hasNotes ? ", notes present" : ""}`;
    }
  });
}

function preloadNearbyImages(image) {
  const urls = new Set();
  for (const profile of image.profiles || []) {
    if (profile.url) urls.add(versionedUrl(profile.url, profile.updated_at));
  }
  if (image.preview_url) urls.add(versionedUrl(image.preview_url, image.preview_updated_at));

  for (const nearby of nearbyImages(image.id)) {
    const selected = selectedProfile(nearby);
    if (selected?.url) {
      urls.add(versionedUrl(selected.url, selected.updated_at));
    } else if (nearby.preview_url) {
      urls.add(versionedUrl(nearby.preview_url, nearby.preview_updated_at));
    }
  }

  for (const url of urls) preloadImage(url);
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
  image.src = url;
  if (state.preloaded.size > 96) {
    state.preloaded = new Set(Array.from(state.preloaded).slice(-64));
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

function togglePublishWizard(force) {
  const show = force ?? els.publishOverlay.hidden;
  if (show) {
    populatePublishWizard();
  }
  els.publishOverlay.hidden = !show;
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
  updatePublishModeText();
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
    outputs += publishProfileIndexes(image).length;
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
  return (
    body.output_format !== (defaults.output_format || "jpg") ||
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
    ? "Changed output settings will rerender selected pictures from the original RAW files."
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
      highlights: clamp(Number(normalized.adjustments?.highlights) || 0, -100, 100),
      shadows: clamp(Number(normalized.adjustments?.shadows) || 0, -100, 100),
      whites: clamp(Number(normalized.adjustments?.whites) || 0, -100, 100),
      blacks: clamp(Number(normalized.adjustments?.blacks) || 0, -100, 100),
      temperature: clamp(Number(normalized.adjustments?.temperature) || 0, -2500, 2500),
      offset: clamp(Number(normalized.adjustments?.offset) || 0, -100, 100),
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

function retouchFromInputs(image = findImage(state.currentId)) {
  const existing = normalizedRetouch(image?.retouch || defaultRetouch());
  return normalizedRetouch({
    adjustments: {
      exposure: Number(els.retouchExposure.value || 0),
      highlights: Number(els.retouchHighlights.value || 0),
      shadows: Number(els.retouchShadows.value || 0),
      whites: Number(els.retouchWhites.value || 0),
      blacks: Number(els.retouchBlacks.value || 0),
      temperature: Number(els.retouchTemperature.value || 0),
      offset: Number(els.retouchOffset.value || 0),
      clarity: Number(els.retouchClarity.value || 0),
    },
    crop: existing.crop,
    rotation_degrees: existing.rotation_degrees,
  });
}

function setRetouchInputs(retouch) {
  const normalized = normalizedRetouch(retouch);
  els.retouchExposure.value = String(normalized.adjustments.exposure);
  els.retouchHighlights.value = String(normalized.adjustments.highlights);
  els.retouchShadows.value = String(normalized.adjustments.shadows);
  els.retouchWhites.value = String(normalized.adjustments.whites);
  els.retouchBlacks.value = String(normalized.adjustments.blacks);
  els.retouchTemperature.value = String(normalized.adjustments.temperature);
  els.retouchOffset.value = String(normalized.adjustments.offset);
  els.retouchClarity.value = String(normalized.adjustments.clarity);
  updateRetouchReadouts(normalized);
}

function updateRetouchReadouts(retouch = retouchFromInputs()) {
  const normalized = normalizedRetouch(retouch);
  els.retouchExposureValue.value = signed(normalized.adjustments.exposure, 2);
  els.retouchHighlightsValue.value = signed(normalized.adjustments.highlights, 0);
  els.retouchShadowsValue.value = signed(normalized.adjustments.shadows, 0);
  els.retouchWhitesValue.value = signed(normalized.adjustments.whites, 0);
  els.retouchBlacksValue.value = signed(normalized.adjustments.blacks, 0);
  els.retouchTemperatureValue.value = `${signed(normalized.adjustments.temperature, 0)}K`;
  els.retouchOffsetValue.value = signed(normalized.adjustments.offset, 0);
  els.retouchClarityValue.value = signed(normalized.adjustments.clarity, 0);
}

function signed(value, digits) {
  const rounded = Number(value || 0).toFixed(digits);
  return Number(rounded) > 0 ? `+${rounded}` : rounded;
}

function isRetouchControlActive() {
  return Boolean(document.activeElement?.closest(".retouch"));
}

function applyLocalRetouch(retouch, options = {}) {
  const image = findImage(state.currentId);
  if (!image) return;
  state.localRetouchDirty = true;
  image.retouch = normalizedRetouch(retouch);
  setRetouchInputs(image.retouch);
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image, selectedProfile(image));
  renderCropOverlay(image);
  renderList(filteredImages());
  renderProfiles(image);
  const selected = selectedProfile(image);
  els.profileState.textContent = selected
    ? `${selected.profile_stem}: ${profileDisplayState(image, selected).text}`
    : "";
  if (options.save !== false) scheduleRetouchSave();
}

function applyDraftRetouch(image, selected) {
  const retouch = cropDraftIsFor(image)
    ? normalizedRetouch({
        ...(image?.retouch || defaultRetouch()),
        rotation_degrees: state.cropDraftRotation,
      })
    : normalizedRetouch(image?.retouch || defaultRetouch());
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
  const brightness = clamp(
    1 + exposure * 0.13 + whites * 0.002 - blacks * 0.0015 + shadows * 0.0015 - highlights * 0.0008,
    0.45,
    1.85,
  );
  const contrast = clamp(1 + clarity * 0.004 + (highlights - shadows) * 0.0008, 0.55, 1.65);
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
  const retouch = cropDraftIsFor(image)
    ? normalizedRetouch({
        ...(image?.retouch || defaultRetouch()),
        rotation_degrees: state.cropDraftRotation,
      })
    : normalizedRetouch(image?.retouch || defaultRetouch());
  const display = profileDisplayState(image, selected);
  const rotating =
    Math.abs(retouch.rotation_degrees) > 0.001 &&
    (cropDraftIsFor(image) ||
      state.localRetouchDirty ||
      display.state === "retouch-queued" ||
      display.state === "retouch-processing");
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

function renderCropOverlay(image) {
  const crop = cropForOverlay(image);
  const visible = Boolean(image && cropDraftIsFor(image) && crop);
  els.cropOverlay.hidden = !visible;
  els.cropBox.hidden = !visible;
  els.cropTools.hidden = !cropDraftIsFor(image);
  updateCropButtons(image);
  updateCropRotationControls();
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
  const imageRect = els.image.getBoundingClientRect();
  const viewerRect = els.viewer.getBoundingClientRect();
  if (imageRect.width <= 1 || imageRect.height <= 1) return;
  els.cropOverlay.style.left = `${imageRect.left - viewerRect.left}px`;
  els.cropOverlay.style.top = `${imageRect.top - viewerRect.top}px`;
  els.cropOverlay.style.width = `${imageRect.width}px`;
  els.cropOverlay.style.height = `${imageRect.height}px`;
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

function clearCropDraftState() {
  state.cropEditing = false;
  state.cropDraft = null;
  state.cropDraftRotation = 0;
  state.cropDraftImageId = null;
  state.cropDrag = null;
  state.cropPointers.clear();
  state.cropTouchGesture = null;
}

function beginCropEditing() {
  const image = findImage(state.currentId);
  if (!image) return;
  if (cropDraftIsFor(image)) return;
  clearRetouchSaveTimer();
  const retouch = normalizedRetouch(image.retouch || defaultRetouch());
  state.cropEditing = true;
  state.cropDraftImageId = image.id;
  state.cropDraft = retouch.crop || defaultCrop();
  state.cropDraftRotation = retouch.rotation_degrees;
  updateCropRotationControls();
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image, selectedProfile(image));
  renderCropOverlay(image);
}

function cancelCropEditing() {
  const image = findImage(state.currentId);
  clearCropDraftState();
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image, selectedProfile(image));
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
  els.cropOk.hidden = !editing;
  els.cropCancel.hidden = !editing;
}

function updateCropRotationControls() {
  els.cropRotation.value = String(clamp(state.cropDraftRotation, -180, 180));
  els.cropRotationValue.value = `${signed(state.cropDraftRotation, 1)}°`;
}

function setCropDraftRotation(value) {
  let image = findImage(state.currentId);
  if (!cropDraftIsFor(image)) {
    beginCropEditing();
    image = findImage(state.currentId);
    if (!cropDraftIsFor(image)) return;
  }
  state.cropDraftRotation = normalizeRotation(value);
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
  const size = Math.min(gesture.crop.width, gesture.crop.height) * scale;
  state.cropDraft = squareCropAround(metrics.center, size);
  state.cropDraftRotation = normalizeRotation(
    gesture.rotation + ((metrics.angle - gesture.startAngle) * 180) / Math.PI,
  );
  updateCropRotationControls();
  applyDraftRetouch(image, selectedProfile(image));
  renderRetouchGrid(image);
  renderCropOverlay(image);
}

function squareCropAround(center, size) {
  const normalizedSize = clamp(size, 0.01, 1);
  return normalizedRetouch({
    crop: {
      x: clamp(center.x, 0, 1) - normalizedSize / 2,
      y: clamp(center.y, 0, 1) - normalizedSize / 2,
      width: normalizedSize,
      height: normalizedSize,
    },
  }).crop;
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
    crop = normalizedRetouch({
      crop: {
        ...drag.crop,
        x: drag.crop.x + dx,
        y: drag.crop.y + dy,
      },
    }).crop;
  } else {
    crop = aspectLockedCrop(drag.crop, drag.handle, dx, dy);
  }
  state.cropDraft = crop;
  renderCropOverlay(findImage(state.currentId));
}

function aspectLockedCrop(start, handle, dx, dy) {
  const anchorX = handle.includes("w") ? start.x + start.width : start.x;
  const anchorY = handle.includes("n") ? start.y + start.height : start.y;
  const signX = handle.includes("w") ? -1 : 1;
  const signY = handle.includes("n") ? -1 : 1;
  const targetWidth = signX > 0 ? start.width + dx : start.width - dx;
  const targetHeight = signY > 0 ? start.height + dy : start.height - dy;
  let size = Math.min(Math.abs(targetWidth), Math.abs(targetHeight));
  size = clamp(size, 0.01, 1);
  if (signX > 0) {
    size = Math.min(size, 1 - anchorX);
  } else {
    size = Math.min(size, anchorX);
  }
  if (signY > 0) {
    size = Math.min(size, 1 - anchorY);
  } else {
    size = Math.min(size, anchorY);
  }
  return normalizedRetouch({
    crop: {
      x: signX > 0 ? anchorX : anchorX - size,
      y: signY > 0 ? anchorY : anchorY - size,
      width: size,
      height: size,
    },
  }).crop;
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
  state.saveQueue = state.saveQueue
    .catch(() => {})
    .then(async () => {
      const response = await fetch(reviewUrl("api/review"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!response.ok) throw new Error(`review ${response.status}`);
      applyState(await response.json());
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
    selected_profile_index: patch.selected_profile_index ?? image.selected_profile_index,
    publish_profile_indexes: patch.publish_profile_indexes ?? publishProfileIndexes(image),
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
      applyState(await response.json());
    });
  return state.saveQueue;
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
  const hasProfile = (image.profiles || []).some((profile) => profile.profile_index === profileIndex);
  if (!hasProfile || image.selected_profile_index === profileIndex) return;
  await saveImageReview(image, { selected_profile_index: profileIndex });
}

async function selectProfileRelative(delta) {
  const image = findImage(state.currentId);
  const profiles = image?.profiles || [];
  if (profiles.length === 0) return;
  const index = profiles.findIndex((profile) => profile.profile_index === image.selected_profile_index);
  const next = (Math.max(0, index) + delta + profiles.length) % profiles.length;
  await saveReview({ selected_profile_index: profiles[next].profile_index });
}

async function toggleSelectedProfilePublish() {
  const image = findImage(state.currentId);
  const profile = selectedProfile(image);
  if (!image || !profile) return;
  await saveReview({ publish_profile_indexes: togglePublishProfile(image, profile.profile_index) });
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

function shouldIgnoreReviewNavigationEvent(event) {
  if (!els.publishOverlay.hidden || !els.shortcutsOverlay.hidden || state.cropEditing) return true;
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
  if (shouldIgnoreReviewNavigationEvent(event)) return;
  event.preventDefault();

  const step = navigationWheelStep(event);
  if (!step) return;

  if (step.axis === "x") {
    move(step.direction > 0 ? 1 : -1).catch((error) => console.error(error));
    return;
  }
  selectProfileRelative(step.direction > 0 ? 1 : -1).catch((error) => console.error(error));
}

function isBackForwardMouseButton(event) {
  return event.button === 3 || event.button === 4;
}

function handleBackForwardMouseDown(event) {
  if (!isBackForwardMouseButton(event) || shouldIgnoreReviewNavigationEvent(event)) return;
  event.preventDefault();
  rateCurrentWithoutAdvance(adjustedCurrentRating(event.button === 4 ? 1 : -1)).catch((error) => console.error(error));
}

function suppressBackForwardMouseDefault(event) {
  if (!isBackForwardMouseButton(event) || shouldIgnoreReviewNavigationEvent(event)) return;
  event.preventDefault();
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
  if (state.cropEditing || !findImage(state.currentId) || !els.image.getAttribute("src")) return false;
  if (event.pointerType !== "touch" && event.button !== 0) return false;
  const target = pointerTargetElement(event);
  return !target?.closest(".crop-overlay, .crop-tools, .retouch-grid, .gesture-feedback, .zoom-loupe");
}

function startZoomHold(event) {
  if (!canStartViewerZoom(event)) return false;
  cancelZoomHold();
  state.zoomLastPoint = { clientX: event.clientX, clientY: event.clientY };
  state.zoomPress = {
    pointerId: event.pointerId,
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
  updateZoomLoupe(point.clientX, point.clientY);
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
  els.viewer.classList.remove("zooming");
  els.zoomLoupe.hidden = true;
  els.zoomLoupe.style.removeProperty("background-image");
  els.zoomLoupe.style.removeProperty("background-size");
  els.zoomLoupe.style.removeProperty("background-position");
  els.zoomLoupe.style.removeProperty("filter");
}

function updateZoomHold(event) {
  if (!state.zoomPress || state.zoomPress.pointerId !== event.pointerId) return;
  state.zoomLastPoint = { clientX: event.clientX, clientY: event.clientY };
  const dx = event.clientX - state.zoomPress.startX;
  const dy = event.clientY - state.zoomPress.startY;
  if (Math.hypot(dx, dy) > ZOOM_MOVE_CANCEL_PX) cancelZoomHold();
}

function updateZoomLoupe(clientX, clientY) {
  const imageRect = els.image.getBoundingClientRect();
  const viewerRect = els.viewer.getBoundingClientRect();
  if (imageRect.width <= 1 || imageRect.height <= 1 || viewerRect.width <= 1 || viewerRect.height <= 1) return;

  state.zoomLastPoint = { clientX, clientY };
  const loupeWidth = els.zoomLoupe.offsetWidth || 180;
  const loupeHeight = els.zoomLoupe.offsetHeight || loupeWidth;
  const left = clamp(clientX - viewerRect.left - loupeWidth / 2, 0, Math.max(0, viewerRect.width - loupeWidth));
  const top = clamp(clientY - viewerRect.top - loupeHeight / 2, 0, Math.max(0, viewerRect.height - loupeHeight));
  const relX = clamp((clientX - imageRect.left) / imageRect.width, 0, 1);
  const relY = clamp((clientY - imageRect.top) / imageRect.height, 0, 1);
  const bgX = loupeWidth / 2 - relX * imageRect.width * ZOOM_SCALE;
  const bgY = loupeHeight / 2 - relY * imageRect.height * ZOOM_SCALE;
  const imageUrl = els.image.currentSrc || els.image.src;
  const imageStyle = window.getComputedStyle(els.image);

  els.zoomLoupe.style.left = `${left}px`;
  els.zoomLoupe.style.top = `${top}px`;
  els.zoomLoupe.style.backgroundImage = `url("${cssUrl(imageUrl)}")`;
  els.zoomLoupe.style.backgroundSize = `${imageRect.width * ZOOM_SCALE}px ${imageRect.height * ZOOM_SCALE}px`;
  els.zoomLoupe.style.backgroundPosition = `${bgX}px ${bgY}px`;
  els.zoomLoupe.style.filter = imageStyle.filter === "none" ? "" : imageStyle.filter;
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
  if (state.zoomActive && state.zoomPointerId === event.pointerId) {
    event.preventDefault();
    updateZoomLoupe(event.clientX, event.clientY);
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
  if (state.zoomActive) stopZoom();
  scheduleViewerSafeAreaUpdate();
  renderRetouchGrid(findImage(state.currentId));
  renderCropOverlay(findImage(state.currentId));
});
els.viewer.addEventListener("pointerdown", startViewerTouch);
els.viewer.addEventListener("pointermove", updateViewerTouch);
els.viewer.addEventListener("pointerup", (event) => {
  endViewerTouch(event).catch((error) => console.error(error));
});
els.viewer.addEventListener("pointercancel", (event) => {
  if (state.zoomActive && state.zoomPointerId === event.pointerId) stopZoom();
  if (state.zoomPress?.pointerId === event.pointerId) cancelZoomHold();
  state.touchGesture = null;
});
els.viewer.addEventListener("contextmenu", (event) => {
  if (state.zoomActive || state.zoomPress || isViewerGestureSurface(event)) event.preventDefault();
});
els.viewer.addEventListener("dragstart", preventNativeViewerAction);
els.viewer.addEventListener("selectstart", preventNativeViewerAction);
els.viewer.addEventListener("touchstart", preventNativeViewerAction, { passive: false });
els.viewer.addEventListener("touchmove", preventNativeViewerAction, { passive: false });
[
  els.retouchExposure,
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
  input.addEventListener("change", () => scheduleRetouchSave());
});
els.retouchReset.addEventListener("click", () => applyLocalRetouch(defaultRetouch()));
els.cropReset.addEventListener("click", clearCropDraft);
els.cropToggle.addEventListener("click", beginCropEditing);
els.cropOk.addEventListener("click", approveCropEditing);
els.cropCancel.addEventListener("click", cancelCropEditing);
els.cropRotation.addEventListener("input", () => setCropDraftRotation(Number(els.cropRotation.value || 0)));
els.cropRotateLeft.addEventListener("click", () => setCropDraftRotation(state.cropDraftRotation - 90));
els.cropRotateRight.addEventListener("click", () => setCropDraftRotation(state.cropDraftRotation + 90));
els.cropBox.addEventListener("pointerdown", startCropDrag);
els.cropBox.addEventListener("pointermove", updateCropDrag);
els.cropBox.addEventListener("pointerup", endCropDrag);
els.cropBox.addEventListener("pointercancel", endCropDrag);
document.querySelectorAll(".retouch label > span").forEach((label) => {
  label.title = "Double-click to reset";
  label.addEventListener("dblclick", (event) => {
    event.preventDefault();
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
  if (event.key !== "Enter") return;
  event.preventDefault();
  clearTimeout(autosaveTimer);
  event.currentTarget.blur();
  saveCurrentIfNeeded().catch((error) => console.error(error));
}

function confirmTagsInput(event) {
  if (event.key !== "Enter") return;
  event.preventDefault();
  clearTimeout(autosaveTimer);
  event.currentTarget.blur();
  move(1).catch((error) => console.error(error));
}

function focusMetadataInput(input) {
  if (isMobileReviewLayout()) {
    setMobileDrawer("metadata");
    requestAnimationFrame(() => {
      input.focus();
      input.select();
    });
    return;
  }
  input.focus();
  input.select();
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
els.mobileDrawerButtons.forEach((button) => {
  button.addEventListener("click", () => toggleMobileDrawer(button.dataset.mobileDrawer));
});
els.publishCancel.addEventListener("click", () => togglePublishWizard(false));
els.publishOverlay.addEventListener("click", (event) => {
  if (event.target === els.publishOverlay) togglePublishWizard(false);
});
els.publishForm.addEventListener("input", updatePublishModeText);
els.publishForm.addEventListener("change", (event) => {
  if (event.target === els.publishSizeMode) {
    syncPublishSizeFields();
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
    applyState(data);
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
    focusMetadataInput(els.tags);
    return;
  }
  if (event.key === "/") {
    event.preventDefault();
    focusMetadataInput(els.notes);
    return;
  }
  if (event.target.closest(".retouch") || event.target.closest(".crop-tools")) return;
  if (event.key === "ArrowRight" || event.key.toLowerCase() === "l" || event.key === "Enter") move(1);
  if (event.key === "ArrowLeft" || event.key.toLowerCase() === "h") move(-1);
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
    toggleSelectedProfilePublish();
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
  if (["1", "2", "3", "4", "5"].includes(event.key)) {
    event.preventDefault();
    rateCurrentAndAdvance(Number(event.key));
  }
  if (["6", "7", "8", "9", "0"].includes(event.key)) {
    event.preventDefault();
    const label = { 6: "red", 7: "yellow", 8: "green", 9: "blue", 0: "purple" }[event.key];
    toggleCurrentLabel(label);
  }
  if (["r", "y", "g", "b", "v", "n"].includes(event.key.toLowerCase())) {
    const label = { r: "red", y: "yellow", g: "green", b: "blue", v: "purple", n: "none" }[event.key.toLowerCase()];
    toggleCurrentLabel(label);
  }
});

els.workspace.addEventListener("wheel", handleNavigationWheel, { passive: false });
els.workspace.addEventListener("mousedown", handleBackForwardMouseDown);
els.workspace.addEventListener("mouseup", suppressBackForwardMouseDefault);
els.workspace.addEventListener("auxclick", suppressBackForwardMouseDefault);

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
window.addEventListener("resize", scheduleViewerSafeAreaUpdate);
document.addEventListener("fullscreenchange", scheduleViewerSafeAreaUpdate);

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
    applyState(JSON.parse(event.data));
  };
  events.onerror = () => {
    els.liveDot.classList.remove("connected");
    els.status.textContent = "Reconnecting...";
  };
}

loadState()
  .then(connectEvents)
  .catch((error) => {
    els.status.textContent = `Disconnected: ${error.message}`;
    setTimeout(() => window.location.reload(), 1500);
  });
