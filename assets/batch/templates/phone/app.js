const overlay = document.getElementById("mf-overlay");
const overlayImage = document.getElementById("mf-overlay-image");
const overlayCaption = document.getElementById("mf-overlay-caption");
const overlayDownload = document.getElementById("mf-overlay-download");
const overlayMeta = document.getElementById("mf-overlay-meta");
const overlayNote = document.getElementById("mf-overlay-note");
const closeButton = document.getElementById("mf-overlay-close");
const nextButton = document.getElementById("mf-overlay-next");
const prevButton = document.getElementById("mf-overlay-prev");
const filterBox = document.getElementById("mf-filter");
const tagFilter = document.getElementById("mf-tag-filter");
const tagList = document.getElementById("mf-tag-list");
const tagClear = document.getElementById("mf-tag-clear");
const filterCount = document.getElementById("mf-filter-count");
const thumbs = Array.from(document.querySelectorAll(".mf-thumb"));
let currentIndex = null;
let visibleThumbs = thumbs;
let availableTags = new Map();
let selectedTags = [];

function currentUrlBase() {
  return `${window.location.pathname}${window.location.search}`;
}

function formatValue(value) {
  const normalized = (value || "").trim();
  return normalized.length > 0 ? normalized : "n/a";
}

function formatExif(button) {
  const focal = formatValue(button.dataset.focal);
  const aperture = formatValue(button.dataset.aperture);
  const shutter = formatValue(button.dataset.shutter);
  const iso = formatValue(button.dataset.iso);
  const camera = formatValue(button.dataset.camera);

  const fields = [
    `Focal length: ${focal}`,
    `Aperture: ${aperture}`,
    `Shutter: ${shutter}`,
    `ISO: ${iso}`,
    `Camera: ${camera}`,
  ];

  return fields.join(" · ");
}

function tagsFor(button) {
  try {
    const tags = JSON.parse(button.dataset.tags || "[]");
    return Array.isArray(tags) ? tags.map(String).filter(Boolean) : [];
  } catch {
    return [];
  }
}

function normalizedTag(value) {
  return String(value || "")
    .trim()
    .toLocaleLowerCase();
}

function tagMatches(button, selected) {
  if (selected.length === 0) {
    return true;
  }
  const buttonTags = new Set(tagsFor(button).map(normalizedTag));
  return selected.some((tag) => buttonTags.has(tag));
}

function setOverlayHash(index) {
  if (index === null) {
    window.history.replaceState(null, "", currentUrlBase());
    return;
  }
  window.history.replaceState(null, "", `${currentUrlBase()}#${index + 1}`);
}

function parseOverlayHashIndex() {
  const hash = window.location.hash.trim();
  if (hash.length <= 1) {
    return null;
  }
  const match = /^#(?:i=|img-)?(\d+)$/i.exec(hash);
  if (!match) {
    return null;
  }
  const oneBased = Number.parseInt(match[1], 10);
  if (!Number.isFinite(oneBased) || oneBased < 1 || oneBased > thumbs.length) {
    return null;
  }
  return oneBased - 1;
}

function openOverlayAt(index) {
  if (index < 0 || index >= thumbs.length) {
    return;
  }

  const button = thumbs[index];
  const full = button.getAttribute("data-full") || "";
  const caption = button.getAttribute("data-caption") || "";
  const note = (button.dataset.note || "").trim();

  overlayImage.src = full;
  overlayImage.alt = caption;
  overlayDownload.href = full;
  overlayCaption.textContent = caption;
  overlayMeta.textContent = formatExif(button);
  overlayNote.textContent = note;
  overlayNote.hidden = note.length === 0;
  currentIndex = index;
  overlay.classList.add("open");
  overlay.setAttribute("aria-hidden", "false");
  setOverlayHash(index);
}

function closeOverlay() {
  overlay.classList.remove("open");
  overlay.setAttribute("aria-hidden", "true");
  overlayImage.removeAttribute("src");
  overlayMeta.textContent = "";
  overlayNote.textContent = "";
  overlayNote.hidden = true;
  currentIndex = null;
  setOverlayHash(null);
}

function moveOverlay(step) {
  if (currentIndex === null || !overlay.classList.contains("open") || visibleThumbs.length === 0) {
    return;
  }

  const currentButton = thumbs[currentIndex];
  const visibleIndex = visibleThumbs.indexOf(currentButton);
  const baseIndex = visibleIndex >= 0 ? visibleIndex : 0;
  const nextVisible = (baseIndex + step + visibleThumbs.length) % visibleThumbs.length;
  openOverlayAt(thumbs.indexOf(visibleThumbs[nextVisible]));
}

function openOverlayFromHash() {
  const target = parseOverlayHashIndex();
  if (target === null) {
    if (currentIndex !== null) {
      closeOverlay();
    }
    return;
  }
  if (target === currentIndex && overlay.classList.contains("open")) {
    return;
  }
  openOverlayAt(target);
}

function setupTagFilter() {
  for (const button of thumbs) {
    for (const tag of tagsFor(button)) {
      const key = normalizedTag(tag);
      if (key && !availableTags.has(key)) {
        availableTags.set(key, tag);
      }
    }
  }
  if (availableTags.size === 0 || !filterBox || !tagFilter || !tagList) {
    return;
  }

  filterBox.hidden = false;
  for (const tag of Array.from(availableTags.values()).sort((a, b) => a.localeCompare(b))) {
    const option = document.createElement("option");
    option.value = tag;
    tagList.appendChild(option);
  }
  applyTagFilter();
}

function committedTagFromInput() {
  const query = normalizedTag(tagFilter?.value || "");
  if (!query) {
    return null;
  }
  if (availableTags.has(query)) {
    return query;
  }
  const exact = Array.from(availableTags.keys()).find((tag) => tag === query);
  if (exact) {
    return exact;
  }
  return Array.from(availableTags.keys()).find((tag) => tag.includes(query)) || null;
}

function addSelectedTag(tag) {
  if (!tag || selectedTags.includes(tag)) {
    return;
  }
  selectedTags.push(tag);
  if (tagFilter) {
    tagFilter.value = "";
  }
  renderSelectedTagPills();
  applyTagFilter();
}

function removeSelectedTag(tag) {
  selectedTags = selectedTags.filter((selected) => selected !== tag);
  renderSelectedTagPills();
  applyTagFilter();
}

function renderSelectedTagPills() {
  if (!filterBox) {
    return;
  }
  filterBox.querySelectorAll(".mf-tag-pill").forEach((pill) => pill.remove());
  const anchor = tagFilter || tagClear || filterCount;
  for (const tag of selectedTags) {
    const pill = document.createElement("button");
    pill.type = "button";
    pill.className = "mf-tag-pill";
    pill.textContent = availableTags.get(tag) || tag;
    pill.setAttribute("aria-label", `Remove ${pill.textContent}`);
    pill.addEventListener("click", () => removeSelectedTag(tag));
    filterBox.insertBefore(pill, anchor);
  }
}

function applyTagFilter() {
  visibleThumbs = [];
  for (const button of thumbs) {
    const visible = tagMatches(button, selectedTags);
    button.hidden = !visible;
    if (visible) {
      visibleThumbs.push(button);
    }
  }
  if (filterCount) {
    const total = thumbs.length;
    filterCount.textContent = selectedTags.length > 0 ? `${visibleThumbs.length}/${total}` : `${total}`;
  }
  if (currentIndex !== null && thumbs[currentIndex]?.hidden) {
    closeOverlay();
  }
}

thumbs.forEach((button, index) => {
  button.addEventListener("click", (event) => {
    event.preventDefault();
    openOverlayAt(index);
  });
});

if (tagFilter) {
  tagFilter.addEventListener("input", () => {
    const exact = normalizedTag(tagFilter.value);
    if (availableTags.has(exact)) {
      addSelectedTag(exact);
    }
  });
  tagFilter.addEventListener("change", () => addSelectedTag(committedTagFromInput()));
  tagFilter.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") {
      return;
    }
    event.preventDefault();
    addSelectedTag(committedTagFromInput());
  });
}

if (tagClear) {
  tagClear.addEventListener("click", () => {
    selectedTags = [];
    tagFilter.value = "";
    renderSelectedTagPills();
    applyTagFilter();
    tagFilter.focus();
  });
}

overlay.addEventListener("click", (event) => {
  if (event.target === overlay || event.target === closeButton) {
    closeOverlay();
  }
});

if (prevButton) {
  prevButton.addEventListener("click", (event) => {
    event.stopPropagation();
    moveOverlay(-1);
  });
}

if (nextButton) {
  nextButton.addEventListener("click", (event) => {
    event.stopPropagation();
    moveOverlay(1);
  });
}

document.addEventListener("keydown", (event) => {
  if (!overlay.classList.contains("open")) {
    return;
  }

  switch (event.key) {
    case "Escape":
      closeOverlay();
      break;
    case "ArrowLeft":
      event.preventDefault();
      moveOverlay(-1);
      break;
    case "ArrowRight":
      event.preventDefault();
      moveOverlay(1);
      break;
    default:
      break;
  }
});

window.addEventListener("hashchange", openOverlayFromHash);
setupTagFilter();
openOverlayFromHash();
