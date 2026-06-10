const state = {
  data: null,
  currentId: null,
  lastInputImageId: null,
  loadedPersistedUi: false,
  saveQueue: Promise.resolve(),
};

const reviewStorageKey = `mini-film-review:${location.host}${location.pathname}`;

const els = {
  status: document.getElementById("status"),
  liveDot: document.getElementById("live-dot"),
  list: document.getElementById("image-list"),
  viewer: document.querySelector(".viewer"),
  image: document.getElementById("main-image"),
  title: document.getElementById("image-title"),
  subtitle: document.getElementById("image-subtitle"),
  profileState: document.getElementById("profile-state"),
  profiles: document.getElementById("profiles"),
  tags: document.getElementById("tags"),
  notes: document.getElementById("notes"),
  publish: document.getElementById("publish"),
  minRating: document.getElementById("min-rating"),
};

async function loadState(keepCurrent = true) {
  const response = await fetch("/api/state", { cache: "no-store" });
  if (!response.ok) throw new Error(`state ${response.status}`);
  applyState(await response.json(), keepCurrent);
}

function applyState(data, keepCurrent = true) {
  state.data = data;
  restorePersistedUiOnce();
  if (!keepCurrent || state.currentId === null || !findImage(state.currentId)) {
    state.currentId = persistedCurrentImageId() || firstReviewableImageId();
  }
  render();
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
  return (state.data?.images || []).find((image) => image.id === id) || null;
}

function render() {
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

function minRating() {
  return Number(els.minRating.value || 0);
}

function passesFilter(image) {
  return Number(image.rating || 0) >= minRating();
}

function filteredImages() {
  return (state.data?.images || []).filter(passesFilter);
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
  const profiles = image.profiles || [];
  const total = profiles.length;
  const done = profiles.filter((profile) => profile.status === "done").length;
  const failed = profiles.filter((profile) => profile.status === "failed").length;
  const processing = profiles.some((profile) => profile.status === "processing");
  const queued = profiles.some((profile) => profile.status === "queued");
  const previewReady = Boolean(image.preview_url);

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
    els.image.src = `${mainUrl}?v=${encodeURIComponent(stamp || "")}`;
    els.image.alt = image.file_name;
  } else {
    els.viewer.classList.remove("has-image");
    els.image.removeAttribute("src");
  }

  renderProfiles(image);
}

function selectedProfile(image) {
  return image.profiles.find((profile) => profile.profile_index === image.selected_profile_index) || image.profiles[0] || null;
}

function renderProfiles(image) {
  els.profiles.replaceChildren();
  image.profiles.forEach((profile) => {
    const cardUrl = profile.url || image.preview_url;
    const card = document.createElement("button");
    card.type = "button";
    card.className = `profile-card${profile.profile_index === image.selected_profile_index ? " active" : ""}${profile.url ? "" : " pending"}`;
    card.addEventListener("click", async () => {
      await saveReview({ selected_profile_index: profile.profile_index });
    });

    if (cardUrl) {
      const img = document.createElement("img");
      const stamp = profile.url ? profile.updated_at : image.preview_updated_at;
      img.src = `${cardUrl}?v=${encodeURIComponent(stamp || "")}`;
      img.alt = profile.profile_stem;
      card.append(img);
    }

    const name = document.createElement("div");
    name.className = "profile-name";
    name.textContent = profile.profile_stem;

    const status = document.createElement("div");
    status.className = "profile-status";
    status.textContent = profile.url ? profile.status : `${profile.status} | preview`;

    card.append(name, status);
    els.profiles.append(card);
  });
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
  const body = reviewRequestBody(image, patch);
  state.saveQueue = state.saveQueue
    .catch(() => {})
    .then(async () => {
      const response = await fetch("/api/review", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!response.ok) throw new Error(`review ${response.status}`);
      applyState(await response.json(), true);
    });
  return state.saveQueue;
}

function reviewRequestBody(image, patch = {}) {
  return {
    image_id: image.id,
    rating: patch.rating ?? image.rating,
    label: patch.label ?? image.label,
    tags: patch.tags ?? currentTags(),
    notes: patch.notes ?? els.notes.value,
    selected_profile_index: patch.selected_profile_index ?? image.selected_profile_index,
  };
}

function saveCurrentIfNeeded() {
  if (state.currentId !== null) {
    saveReview().catch((error) => console.error(error));
  }
}

function move(delta) {
  const images = filteredImages();
  const index = images.findIndex((image) => image.id === state.currentId);
  if (index < 0) return;
  const next = Math.max(0, Math.min(images.length - 1, index + delta));
  if (next !== index) {
    saveCurrentIfNeeded();
    state.currentId = images[next].id;
    persistUi();
    render();
  }
}

document.querySelectorAll("[data-rating]").forEach((button) => {
  button.addEventListener("click", () => saveReview({ rating: Number(button.dataset.rating) }));
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
    const response = await fetch("/api/publish", {
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
  if (event.key === "ArrowRight" || event.key.toLowerCase() === "l" || event.key === "Enter") move(1);
  if (event.key === "ArrowLeft" || event.key.toLowerCase() === "h") move(-1);
  if (["0", "1", "2", "3", "4", "5"].includes(event.key)) {
    saveReview({ rating: Number(event.key) }).then(() => move(1));
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
  const body = JSON.stringify(reviewRequestBody(image));
  navigator.sendBeacon("/api/review", new Blob([body], { type: "application/json" }));
});

function connectEvents() {
  const events = new EventSource("/api/events");
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
