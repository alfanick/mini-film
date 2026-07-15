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
  ["4:3", "4:3"],
  ["5:4", "5:4"],
  ["a3-a4", "A3/A4", "A3/A4 portrait"],
  ["1:1", "1:1"],
  ["16:10", "16:10"],
  ["21:9", "21:9"],
  ["3:1", "3:1"],
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
      ),
      h(
        "main",
        { class: "workspace" },
        h(
          "section",
          { class: "viewer" },
          h("div", { id: "empty", class: "empty" }, "Waiting for pictures"),
          h("img", { id: "main-image", alt: "", draggable: false, decoding: "async", fetchpriority: "high" }),
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
    ),
    h(ShortcutsOverlay),
    h("div", { id: "command-invocation-overlay", class: "command-invocation-overlay", hidden: true }),
    h("div", { id: "profile-info-overlay", class: "profile-info-overlay", hidden: true }),
    h(PublishOverlay),
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
        label: "Temperature",
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
          "Click a profile thumbnail to preview it; use its checkbox to include it in publishing. Double-click or double-tap a profile to publish only that profile.",
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
        [["Space"], "Include or skip the selected profile when publishing."],
        [["Double-click"], "Publish only that profile thumbnail and exclude the other profiles."],
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
    [
      "Retouch",
      [
        [["Double-click"], "Double-click a retouch control name to reset that value."],
        [["Crop", "OK"], "Open crop/rotate, adjust the frame, then apply it with OK."],
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
                  "a",
                  {
                    href: link,
                    target: "_blank",
                    rel: "noopener noreferrer",
                    class: "publish-gallery-link",
                  },
                  galleryLinks.length > 1 ? `Gallery ${index + 1}` : "Open gallery",
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
  preactRender(
    h(ImageList, {
      images: images.map((image) => {
        const captureDay = imageCaptureDisplay(image, lastCaptureDay);
        lastCaptureDay = captureDay.day;
        return { ...image, capture_time: captureDay.text };
      }),
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
    const thumbnailUrl = image.thumbnail_url || image.preview_url;
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
  });
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
    if (els.image.getAttribute("src") !== nextSrc) stopZoom();
    els.image.src = nextSrc;
    els.image.alt = image.file_name;
  } else {
    stopZoom();
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
  }

  applyDraftRetouch(image, selected);
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
  const profiles = image?.profiles || [];
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
  if (isDirectCompressedImage(image)) return 0;
  if (profilesAreImplicitOnly(image)) return 0;
  return image ? image.profiles?.length || 0 : state.data?.profiles?.length || 0;
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
  if (profilesAreImplicitOnly(image) || isDirectCompressedImage(image)) {
    preactRender(null, els.profiles);
    return;
  }
  preactRender(
    h(ProfileList, {
      image,
      onSelect: async (profile) => {
        await saveReview({ selected_profile_index: profile.profile_index });
      },
      onTogglePublish: async (profile) => {
        await saveReview({ publish_profile_indexes: togglePublishProfile(image, profile.profile_index) });
      },
      onSoloPublish: async (profile) => {
        await saveReview({
          selected_profile_index: profile.profile_index,
          publish_profile_indexes: [profile.profile_index],
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

function ProfileList({ image, onSelect, onTogglePublish, onSoloPublish }) {
  if (!image) return null;
  const publishIndexes = new Set(publishProfileIndexes(image));
  const previewProfile = selectedProfile(image);
  const profiles = image.profiles || [];
  const canSoloPublish = profiles.length > 1;
  return profiles.map((profile) => {
    const displayName = profileDisplayName(profile);
    const cardUrl = profile.url || image.preview_url;
    const publishSelected = publishIndexes.has(profile.profile_index);
    const display = profileDisplayState(image, profile);
    const isPortrait = isPortraitRenderProfile(profile);
    const sourceStatus = profile.url ? display.text : `${display.text} | preview`;
    const classes = [
      "profile-card",
      profile.profile_index === previewProfile?.profile_index ? "active" : "",
      profile.url ? "" : "pending",
      isPortrait ? "portrait" : "",
      display.state,
      publishSelected ? "publish-selected" : "publish-excluded",
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
            if (!canSoloPublish) return;
            event.preventDefault();
            onSoloPublish(profile).catch((error) => console.error(error));
          },
          onPointerUp: (event) => {
            if (!canSoloPublish || event.pointerType === "mouse") return;
            const now = Date.now();
            const sameProfile = profileDoubleTap.profileIndex === profile.profile_index;
            const isDoubleTap = sameProfile && now - profileDoubleTap.at < 450;
            profileDoubleTap.profileIndex = profile.profile_index;
            profileDoubleTap.at = now;
            if (!isDoubleTap) return;
            event.preventDefault();
            onSoloPublish(profile).catch((error) => console.error(error));
          },
        },
        h("input", {
          type: "checkbox",
          class: "profile-publish",
          checked: publishSelected,
          title: publishSelected ? "Included in publish" : "Skipped by publish",
          "aria-label": `Publish ${displayName}`,
          onClick: (event) => event.stopPropagation(),
          onChange: (event) => {
            event.stopPropagation();
            onTogglePublish(profile).catch((error) => console.error(error));
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
          `${sourceStatus} | ${publishSelected ? "publish" : "skip"}`,
        ),
      ),
      profile.url
        ? h(
            "a",
            {
              class: "profile-download",
              href: versionedUrl(profile.url, profile.updated_at),
              download: profileDownloadName(image, profile),
              title: `Download rendered ${displayName}`,
              "aria-label": `Download rendered ${displayName}`,
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
    grain_engine: els.publishGrainEngine.value,
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
  return (
    body.output_format !== (defaults.output_format || "jpg") ||
    body.grain_engine !== (defaults.grain_engine || "legacy") ||
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
    highlights: clamp(base.highlights + adjustments.highlights, -100, 100),
    shadows: clamp(base.shadows + adjustments.shadows, -100, 100),
    whites: clamp(base.whites + adjustments.whites, -100, 100),
    blacks: clamp(base.blacks + adjustments.blacks, -100, 100),
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
      highlights: Number(els.retouchHighlights.value || 0) - base.highlights,
      shadows: Number(els.retouchShadows.value || 0) - base.shadows,
      whites: Number(els.retouchWhites.value || 0) - base.whites,
      blacks: Number(els.retouchBlacks.value || 0) - base.blacks,
      temperature: retouchTemperatureDeltaFromInput(image),
      offset: retouchOffsetDeltaFromInput(image),
      clarity: Number(els.retouchClarity.value || 0),
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
  els.retouchHighlights.defaultValue = String(base.highlights);
  els.retouchShadows.defaultValue = String(base.shadows);
  els.retouchWhites.defaultValue = String(base.whites);
  els.retouchBlacks.defaultValue = String(base.blacks);
  els.retouchExposure.value = String(tonalValues.exposure);
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
  els.retouchClarity.value = String(normalized.adjustments.clarity);
  updateRetouchReadouts(normalized, image);
}

function updateRetouchReadouts(retouch = retouchFromInputs(), image = findImage(state.currentId)) {
  const normalized = normalizedRetouch(retouch);
  const tonalValues = retouchTonalInputValues(image, normalized);
  els.retouchExposureValue.value = signed(tonalValues.exposure, 2);
  els.retouchHighlightsValue.value = signed(tonalValues.highlights, 0);
  els.retouchShadowsValue.value = signed(tonalValues.shadows, 0);
  els.retouchWhitesValue.value = signed(tonalValues.whites, 0);
  els.retouchBlacksValue.value = signed(tonalValues.blacks, 0);
  const temperature = Math.round(retouchTemperatureInputValue(image, normalized.adjustments.temperature));
  els.retouchTemperatureValue.value = `${asShotWhiteBalanceTemperature(image) === null ? signed(temperature, 0) : temperature}K`;
  const offset = Math.round(retouchOffsetInputValue(image, normalized.adjustments.offset));
  els.retouchOffsetValue.value = signed(offset, 0);
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
  image.retouch = retouchForImage(image, retouch);
  setRetouchInputs(image.retouch, image);
  applyDraftRetouch(image, selectedProfile(image));
  scheduleHistogramRender({ debounce: true });
  renderRetouchGrid(image, selectedProfile(image));
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
  renderCropOverlay(image);
  if (els.cropSourceImage.complete && els.cropSourceImage.naturalWidth > 0) initializeCropGeometry();
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

function cropRatioBase(key = state.cropRatioKey) {
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
  return state.cropRatioRotated ? 1 / base : base;
}

function cropPixelRatio(crop, rotation = state.cropDraftRotation) {
  const source = cropSourceDimensions();
  if (!source || !crop) return 1;
  const safe = rotatedSafeDimensions(source.width, source.height, rotation);
  return (crop.width * safe.width) / Math.max(1, crop.height * safe.height);
}

function ratiosMatch(left, right) {
  return Math.abs(Math.log(Math.max(0.0001, left) / Math.max(0.0001, right))) < 0.004;
}

function inferCropRatio(crop) {
  const actual = cropPixelRatio(crop);
  for (const [key] of CROP_RATIO_PRESETS) {
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
  state.cropRatioKey = "current";
  state.cropRatioBase = actual;
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
  if (!source || !Number.isFinite(ratio) || ratio <= 0) return normalizeCropRect(crop);
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
  if (!source || !geometry || !Number.isFinite(ratio) || ratio <= 0) {
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
  if (!state.cropGeometryInitialized || ratiosMatch(cropTargetRatio(), 1)) return;
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
  const normalizedRatio = (cropTargetRatio() * safe.height) / safe.width;
  const anchorX = handle.includes("w") ? start.x + start.width : start.x;
  const anchorY = handle.includes("n") ? start.y + start.height : start.y;
  const signX = handle.includes("w") ? -1 : 1;
  const signY = handle.includes("n") ? -1 : 1;
  const targetWidth = signX > 0 ? start.width + dx : start.width - dx;
  const targetHeight = signY > 0 ? start.height + dy : start.height - dy;
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
    publish_profile_indexes: patch.publish_profile_indexes ?? publishProfileIndexes(image),
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
  const profiles = image?.profiles || [];
  if (profiles.length === 0) return;
  const index = profiles.findIndex((profile) => profile.profile_index === image.selected_profile_index);
  const next = (Math.max(0, index) + delta + profiles.length) % profiles.length;
  await saveReview({ selected_profile_index: profiles[next].profile_index });
}

async function toggleSelectedProfilePublish() {
  const image = findImage(state.currentId);
  if (profilesAreImplicitOnly(image)) return;
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

function shouldIgnoreNavigationWheel(event) {
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
  if (state.zoomActive || state.zoomFullActive) stopZoom();
  scheduleHistogramRender();
  scheduleViewerSafeAreaUpdate();
  renderRetouchGrid(findImage(state.currentId));
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
  if (event.target.closest(".retouch") || event.target.closest(".crop-tools")) return;
  if (plainShortcut && shortcutKey === "h") {
    event.preventDefault();
    toggleHistogram();
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
