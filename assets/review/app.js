const state = {
  data: null,
  currentId: null,
  lastInputImageId: null,
  loadedPersistedUi: false,
  saveQueue: Promise.resolve(),
  preloaded: new Set(),
};

const reviewStorageKey = `mini-film-review:${location.host}${location.pathname}`;
const reviewClientIdKey = `${reviewStorageKey}:client-id`;
const reviewClientId = persistedClientId();

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
};

const wideProfilesQuery = window.matchMedia("(min-width: 1280px) and (min-height: 620px)");

function reviewUrl(path) {
  return path.replace(/^\/+/, "");
}

function persistedClientId() {
  try {
    const existing = localStorage.getItem(reviewClientIdKey);
    if (existing) return existing;
    const generated = globalThis.crypto?.randomUUID
      ? globalThis.crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    localStorage.setItem(reviewClientIdKey, generated);
    return generated;
  } catch {
    return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }
}

async function loadState(keepCurrent = true) {
  const response = await fetch(reviewUrl("api/state"), { cache: "no-store" });
  if (!response.ok) throw new Error(`state ${response.status}`);
  applyState(await response.json(), keepCurrent);
}

function applyState(data, keepCurrent = true) {
  const collaborativeAdvance = keepCurrent ? collaborativeAdvanceForIncomingState(data) : null;
  state.data = data;
  restorePersistedUiOnce();
  if (!keepCurrent || state.currentId === null || !findImage(state.currentId)) {
    state.currentId = persistedCurrentImageId() || firstReviewableImageId();
  }
  if (collaborativeAdvance && state.currentId === collaborativeAdvance.imageId) {
    applyCollaborativeAdvance(collaborativeAdvance);
    return;
  }
  render();
}

function collaborativeAdvanceForIncomingState(data) {
  if (!state.data || state.currentId === null) return null;
  const before = findImageInData(state.data, state.currentId);
  const after = findImageInData(data, state.currentId);
  if (!before || !after) return null;
  if (!after.advance_token || after.advance_token === before.advance_token) return null;
  if (after.advance_client_id === reviewClientId) return null;
  const advance = nextReviewAdvanceForData(state.data, state.currentId);
  return {
    ...advance,
    imageId: state.currentId,
    carryProfileIndex: selectedProfileForImage(before)?.profile_index,
  };
}

function applyCollaborativeAdvance(advance) {
  if (advance.kind === "next") {
    advanceToImage(advance.id, advance.carryProfileIndex).catch((error) => console.error(error));
    return;
  }
  if (advance.kind === "next-pass") {
    els.minRating.value = String(Math.min(5, minRating() + 1));
    const nextId = firstReviewableImageId();
    advanceToImage(nextId, advance.carryProfileIndex).catch((error) => console.error(error));
  }
}

function restorePersistedUiOnce() {
  if (state.loadedPersistedUi) return;
  state.loadedPersistedUi = true;
  const saved = readPersistedUi();
  if (!saved) return;
  if (saved.minRating !== undefined) {
    els.minRating.value = String(Math.max(0, Math.min(5, Number(saved.minRating) || 0)));
  }
  if (saved.currentId !== undefined) {
    state.currentId = Number(saved.currentId) || null;
  }
}

function readPersistedUi() {
  try {
    const raw = localStorage.getItem(reviewStorageKey);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

function persistUi() {
  try {
    localStorage.setItem(
      reviewStorageKey,
      JSON.stringify({
        currentId: state.currentId,
        minRating: minRating(),
        updatedAt: new Date().toISOString(),
      }),
    );
  } catch {
    // Local storage can be unavailable in private browsing modes.
  }
}

function persistedCurrentImageId() {
  const saved = readPersistedUi();
  const id = Number(saved?.currentId);
  return id && findImage(id) && passesFilter(findImage(id)) ? id : null;
}

function firstReviewableImageId() {
  const images = filteredImages();
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
  els.status.textContent = `${images.length}/${total} pictures | ${state.data?.profiles?.length || 0} profiles`;
  renderList(images);
  let current = findImage(state.currentId);
  if (current && !passesFilter(current)) current = null;
  if (!current) {
    state.currentId = firstReviewableImageId();
    current = findImage(state.currentId);
  }
  renderCurrent(current);
  persistUi();
}

function syncProfilesPlacement() {
  const shouldUseRail = wideProfilesQuery.matches;
  const parent = els.profiles.parentElement;
  if (shouldUseRail && parent !== els.workspace) {
    els.workspace.append(els.profiles);
    return;
  }
  if (!shouldUseRail && parent !== els.panel) {
    els.panel.insertBefore(els.profiles, els.controls);
  }
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
    button.addEventListener("click", () => {
      saveCurrentIfNeeded();
      state.currentId = image.id;
      persistUi();
      render();
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
  return (image?.profiles || []).find((profile) => profile.profile_index === image.selected_profile_index) || image?.profiles?.[0] || null;
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
      applyState(await response.json(), true);
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
    client_id: reviewClientId,
    advance_after_update: Boolean(patch.advance_after_update),
  };
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
    await advanceToImage(images[next].id, carryProfileIndex);
  }
}

async function rateCurrentAndAdvance(rating) {
  const current = findImage(state.currentId);
  const carryProfileIndex = selectedProfile(current)?.profile_index;
  const advance = nextReviewAdvance();
  await saveReview({ rating, advance_after_update: true });
  if (advance.kind === "next" && findImage(advance.id) && passesFilter(findImage(advance.id))) {
    await advanceToImage(advance.id, carryProfileIndex);
    return;
  }
  if (advance.kind === "next-pass") {
    els.minRating.value = String(Math.min(5, minRating() + 1));
    const nextId = firstReviewableImageId();
    await advanceToImage(nextId, carryProfileIndex);
  }
}

async function advanceToImage(imageId, carryProfileIndex) {
  state.currentId = imageId;
  persistUi();
  render();
  await carrySelectedProfileToImage(imageId, carryProfileIndex);
}

async function carrySelectedProfileToImage(imageId, profileIndex) {
  const image = findImage(imageId);
  if (!image || profileIndex === undefined || profileIndex === null) return;
  const hasProfile = (image.profiles || []).some((profile) => profile.profile_index === profileIndex);
  if (!hasProfile || image.selected_profile_index === profileIndex) return;
  await saveImageReview(image, { selected_profile_index: profileIndex });
}

function nextReviewAdvance() {
  return nextReviewAdvanceForData(state.data, state.currentId);
}

function nextReviewAdvanceForData(data, imageId) {
  const images = filteredImagesFromData(data);
  const index = images.findIndex((image) => image.id === imageId);
  if (index < 0) return { kind: "next", id: firstReviewableImageId() };
  if (index + 1 < images.length) return { kind: "next", id: images[index + 1].id };
  return { kind: "next-pass" };
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
  button.addEventListener("click", () => saveReview({ label: button.dataset.label }));
});

els.tags.addEventListener("change", () => saveReview());
els.tags.addEventListener("blur", () => saveReview());
els.tags.addEventListener("input", scheduleAutosave);
els.notes.addEventListener("change", () => saveReview());
els.notes.addEventListener("blur", () => saveReview());
els.notes.addEventListener("input", scheduleAutosave);
els.minRating.addEventListener("change", () => {
  persistUi();
  render();
});

let autosaveTimer = null;
function scheduleAutosave() {
  clearTimeout(autosaveTimer);
  autosaveTimer = setTimeout(() => saveCurrentIfNeeded(), 500);
}

els.publish.addEventListener("click", async () => {
  els.publish.disabled = true;
  els.publish.textContent = "Publishing";
  try {
    const response = await fetch(reviewUrl("api/publish"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ min_rating: minRating() }),
    });
    const report = await response.json();
    els.status.textContent = `Published ${report.linked} links, ${report.skipped} skipped, rating >= ${report.min_rating}`;
  } finally {
    els.publish.disabled = false;
    els.publish.textContent = "Publish";
  }
});

window.addEventListener("keydown", (event) => {
  if (event.target === els.tags) return;
  if (event.target === els.notes) return;
  if (event.target === els.minRating) return;
  if (event.key === "ArrowRight" || event.key.toLowerCase() === "l" || event.key === "Enter") move(1);
  if (event.key === "ArrowLeft" || event.key.toLowerCase() === "h") move(-1);
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
  if (["0", "1", "2", "3", "4", "5"].includes(event.key)) {
    event.preventDefault();
    rateCurrentAndAdvance(Number(event.key));
  }
  if (["r", "y", "g", "b", "v", "n"].includes(event.key.toLowerCase())) {
    const label = { r: "red", y: "yellow", g: "green", b: "blue", v: "purple", n: "none" }[event.key.toLowerCase()];
    saveReview({ label });
  }
});

window.addEventListener("beforeunload", () => {
  persistUi();
  const image = findImage(state.currentId);
  if (!image || !navigator.sendBeacon) return;
  const body = JSON.stringify(reviewRequestBody(image, {}, { useInputs: true }));
  navigator.sendBeacon(reviewUrl("api/review"), new Blob([body], { type: "application/json" }));
});

wideProfilesQuery.addEventListener("change", syncProfilesPlacement);

function connectEvents() {
  const events = new EventSource(reviewUrl("api/events"));
  events.onopen = () => {
    els.liveDot.classList.add("connected");
  };
  events.onmessage = (event) => {
    els.liveDot.classList.add("connected");
    applyState(JSON.parse(event.data), true);
  };
  events.onerror = () => {
    els.liveDot.classList.remove("connected");
    els.status.textContent = "Reconnecting...";
  };
}

loadState(false)
  .then(connectEvents)
  .catch((error) => {
    els.status.textContent = `Disconnected: ${error.message}`;
    setTimeout(() => window.location.reload(), 1500);
  });
