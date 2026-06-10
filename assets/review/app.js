const state = {
  data: null,
  currentId: null,
  lastInputImageId: null,
  saveQueue: Promise.resolve(),
  preloaded: new Set(),
  viewerSafeAreaObserver: null,
};

const els = {
  status: document.getElementById("status"),
  liveDot: document.getElementById("live-dot"),
  list: document.getElementById("image-list"),
  workspace: document.querySelector(".workspace"),
  viewer: document.querySelector(".viewer"),
  panel: document.querySelector(".panel"),
  image: document.getElementById("main-image"),
  title: document.getElementById("image-title"),
  subtitle: document.getElementById("image-subtitle"),
  profileState: document.getElementById("profile-state"),
  profiles: document.getElementById("profiles"),
  controls: document.querySelector(".controls"),
  tags: document.getElementById("tags"),
  notes: document.getElementById("notes"),
  publish: document.getElementById("publish"),
  minRating: document.getElementById("min-rating"),
  app: document.querySelector(".app"),
  shortcutsHelp: document.getElementById("shortcuts-help"),
  shortcutsOverlay: document.getElementById("shortcuts-overlay"),
  shortcutsClose: document.getElementById("shortcuts-close"),
  appVersion: document.getElementById("app-version"),
  publishOverlay: document.getElementById("publish-overlay"),
  publishForm: document.getElementById("publish-form"),
  publishCancel: document.getElementById("publish-cancel"),
  publishSubmit: document.getElementById("publish-submit"),
  publishStatus: document.getElementById("publish-status"),
  publishMode: document.getElementById("publish-mode"),
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

const wideProfilesQuery = window.matchMedia("(min-width: 1280px) and (min-height: 620px)");

function reviewUrl(path) {
  return path.replace(/^\/+/, "");
}

async function loadState() {
  const response = await fetch(reviewUrl("api/state"), { cache: "no-store" });
  if (!response.ok) throw new Error(`state ${response.status}`);
  applyState(await response.json());
}

function applyState(data) {
  state.data = data;
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

function updatePublishStatus() {
  const job = latestPublishJob();
  if (!job) {
    els.publishStatus.textContent = publishWouldRerender()
      ? "Changed output settings will rerender from original RAWs."
      : "Default settings will link existing reviewed outputs.";
    els.publishSubmit.disabled = false;
    return;
  }
  els.publishStatus.replaceChildren();
  if (job.status === "running") {
    els.publishSubmit.disabled = true;
    const percent = publishProgressPercent(job);
    const text = document.createElement("div");
    text.textContent = `Publishing ${job.album}: ${percent}% ${job.step || "publish"}${job.current ? ` | ${job.current}` : ""}`;
    const bar = document.createElement("div");
    bar.className = "publish-progress";
    const fill = document.createElement("span");
    fill.style.width = `${percent}%`;
    bar.append(fill);
    const counts = document.createElement("div");
    counts.className = "publish-progress-counts";
    counts.textContent = `${job.processed || 0}/${job.total || 0} outputs | linked ${job.linked || 0} | skipped ${job.skipped || 0} | galleries ${job.galleries || 0}`;
    els.publishStatus.append(text, bar, counts);
  } else if (job.status === "done") {
    els.publishSubmit.disabled = false;
    els.publishStatus.textContent = `Published ${job.linked} files to ${job.album}; skipped ${job.skipped}; galleries ${job.galleries}.`;
  } else {
    els.publishSubmit.disabled = false;
    els.publishStatus.textContent = `Publish failed: ${job.error || "unknown error"}`;
  }
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
  els.list.replaceChildren();
  for (const image of images) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `image-row${image.id === state.currentId ? " active" : ""}`;
    button.addEventListener("click", async () => {
      const carryProfileIndex = selectedProfile(findImage(state.currentId))?.profile_index;
      await saveCurrentIfNeeded();
      await updateSharedUi({ current_image_id: image.id, min_rating: minRating() });
      await carrySelectedProfileToImage(image.id, carryProfileIndex);
    });

    const title = document.createElement("div");
    title.className = "image-row-title";
    title.textContent = image.file_name;

    const meta = document.createElement("div");
    meta.className = "image-row-meta";
    const progress = renderProgressSummary(image);
    meta.textContent = `rating ${image.rating} ${image.label !== "none" ? image.label : ""} | ${progress.text}`;

    const indicator = document.createElement("span");
    indicator.className = `image-row-indicator ${progress.state}`;
    indicator.title = progress.title;
    indicator.setAttribute("aria-label", progress.title);

    button.append(title, meta, indicator);
    els.list.append(button);
  }
}

function renderProgressSummary(image) {
  const publishIndexes = new Set(publishProfileIndexes(image));
  const profiles = (image.profiles || []).filter((profile) => publishIndexes.has(profile.profile_index));
  const total = profiles.length;
  const done = profiles.filter((profile) => profile.status === "done").length;
  const failed = profiles.filter((profile) => profile.status === "failed").length;
  const processing = profiles.some((profile) => profile.status === "processing");
  const queued = profiles.some((profile) => profile.status === "queued");
  const previewReady = Boolean(image.preview_url);

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
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
    els.title.textContent = "";
    els.subtitle.textContent = "";
    els.profileState.textContent = "";
    els.profiles.replaceChildren();
    els.tags.value = "";
    els.notes.value = "";
    state.lastInputImageId = null;
    setActiveReviewButtons(null);
    return;
  }

  const selected = selectedProfile(image);
  const mainUrl = selected?.url || image.preview_url;
  const previewNote = selected?.url ? "" : image.preview_url ? " | camera preview" : "";
  els.title.textContent = image.file_name;
  els.subtitle.textContent = `${image.relative_path} | rating ${image.rating}`;
  els.profileState.textContent = selected ? `${selected.profile_stem}: ${selected.status}${previewNote}` : "";
  const imageChanged = state.lastInputImageId !== image.id;
  if (imageChanged || document.activeElement !== els.tags) {
    els.tags.value = image.tags.join(", ");
  }
  if (imageChanged || document.activeElement !== els.notes) {
    els.notes.value = image.notes || "";
  }
  state.lastInputImageId = image.id;
  setActiveReviewButtons(image);

  if (mainUrl) {
    els.viewer.classList.add("has-image");
    const stamp = selected?.url ? selected.updated_at : image.preview_updated_at;
    els.image.src = versionedUrl(mainUrl, stamp);
    els.image.alt = image.file_name;
  } else {
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
  }

  renderProfiles(image);
  preloadNearbyImages(image);
}

function selectedProfile(image) {
  return selectedProfileForImage(image);
}

function selectedProfileForImage(image) {
  return (
    (image?.profiles || []).find((profile) => profile.profile_index === image.selected_profile_index) ||
    image?.profiles?.[0] ||
    null
  );
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
  els.profiles.replaceChildren();
  const publishIndexes = new Set(publishProfileIndexes(image));
  image.profiles.forEach((profile) => {
    const cardUrl = profile.url || image.preview_url;
    const publishSelected = publishIndexes.has(profile.profile_index);
    const card = document.createElement("button");
    card.type = "button";
    card.className = [
      "profile-card",
      profile.profile_index === image.selected_profile_index ? "active" : "",
      profile.url ? "" : "pending",
      publishSelected ? "publish-selected" : "publish-excluded",
    ]
      .filter(Boolean)
      .join(" ");
    card.addEventListener("click", async () => {
      await saveReview({ selected_profile_index: profile.profile_index });
    });

    const publish = document.createElement("input");
    publish.type = "checkbox";
    publish.className = "profile-publish";
    publish.checked = publishSelected;
    publish.title = publishSelected ? "Included in publish" : "Skipped by publish";
    publish.setAttribute("aria-label", `Publish ${profile.profile_stem}`);
    publish.addEventListener("click", (event) => event.stopPropagation());
    publish.addEventListener("change", async (event) => {
      event.stopPropagation();
      await saveReview({ publish_profile_indexes: togglePublishProfile(image, profile.profile_index) });
    });
    card.append(publish);

    if (cardUrl) {
      const img = document.createElement("img");
      const stamp = profile.url ? profile.updated_at : image.preview_updated_at;
      img.src = versionedUrl(cardUrl, stamp);
      img.alt = profile.profile_stem;
      img.addEventListener("load", () => {
        card.classList.toggle("portrait", img.naturalHeight > img.naturalWidth);
      });
      card.append(img);
    }

    const name = document.createElement("div");
    name.className = "profile-name";
    name.textContent = profile.profile_stem;

    const status = document.createElement("div");
    status.className = "profile-status";
    const sourceStatus = profile.url ? profile.status : `${profile.status} | preview`;
    status.textContent = `${sourceStatus} | ${publishSelected ? "publish" : "skip"}`;

    card.append(name, status);
    els.profiles.append(card);
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
  updatePublishStatus();
}

function setActiveReviewButtons(image) {
  document.querySelectorAll("[data-rating]").forEach((button) => {
    button.classList.toggle("active", Number(image?.rating || 0) === Number(button.dataset.rating));
  });
  document.querySelectorAll("[data-label]").forEach((button) => {
    button.classList.toggle("active", (image?.label || "none") === button.dataset.label);
  });
}

function currentTags() {
  return els.tags.value
    .split(/[,\s]+/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

async function saveReview(patch = {}) {
  const image = findImage(state.currentId);
  if (!image) return;
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
    label: patch.label ?? image.label,
    tags: patch.tags ?? (options.useInputs ? currentTags() : image.tags || []),
    notes: patch.notes ?? (options.useInputs ? els.notes.value : image.notes || ""),
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
  saveReview({ label: image.label === label ? "none" : label });
}

function adjustedCurrentRating(delta) {
  const image = findImage(state.currentId);
  const rating = Number(image?.rating || 0) + delta;
  return Math.max(0, Math.min(5, rating));
}

document.querySelectorAll("[data-rating]").forEach((button) => {
  button.addEventListener("click", () => rateCurrentAndAdvance(Number(button.dataset.rating)));
});

document.querySelectorAll("[data-label]").forEach((button) => {
  button.addEventListener("click", () => {
    const label = button.dataset.label;
    if (label === "none") {
      saveReview({ label });
    } else {
      toggleCurrentLabel(label);
    }
  });
});

els.tags.addEventListener("change", () => saveReview());
els.tags.addEventListener("blur", () => saveReview());
els.tags.addEventListener("input", scheduleAutosave);
els.notes.addEventListener("change", () => saveReview());
els.notes.addEventListener("blur", () => saveReview());
els.notes.addEventListener("input", scheduleAutosave);
els.minRating.addEventListener("change", () => {
  updateSharedUi({ current_image_id: state.currentId, min_rating: minRating() }).catch((error) => console.error(error));
});

let autosaveTimer = null;
function scheduleAutosave() {
  clearTimeout(autosaveTimer);
  autosaveTimer = setTimeout(() => saveCurrentIfNeeded(), 500);
}

els.publish.addEventListener("click", () => togglePublishWizard(true));
els.publishCancel.addEventListener("click", () => togglePublishWizard(false));
els.publishOverlay.addEventListener("click", (event) => {
  if (event.target === els.publishOverlay) togglePublishWizard(false);
});
els.publishSizeMode.addEventListener("change", () => {
  syncPublishSizeFields();
  updatePublishModeText();
});
[
  els.publishOutputFormat,
  els.publishLongEdge,
  els.publishMaxWidth,
  els.publishMaxHeight,
  els.publishResize,
  els.publishJpgQuality,
  els.publishJpegSubsampling,
  els.publishProgressive,
  els.publishStripMetadata,
].forEach((element) => {
  element.addEventListener("input", updatePublishModeText);
  element.addEventListener("change", updatePublishModeText);
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
  if (event.target === els.tags) return;
  if (event.target === els.notes) return;
  if (event.target === els.minRating) return;
  if (!els.shortcutsOverlay.hidden) {
    if (event.key === "Escape" || event.key === "?" || (event.key === "/" && event.shiftKey)) {
      event.preventDefault();
      toggleShortcuts(false);
    }
    return;
  }
  if (event.key === "?" || (event.key === "/" && event.shiftKey)) {
    event.preventDefault();
    toggleShortcuts(true);
    return;
  }
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
    if (label === "none") {
      saveReview({ label });
    } else {
      toggleCurrentLabel(label);
    }
  }
});

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
