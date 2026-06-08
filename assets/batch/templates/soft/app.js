const overlay = document.getElementById("mf-overlay");
const overlayImage = document.getElementById("mf-overlay-image");
const overlayCaption = document.getElementById("mf-overlay-caption");
const overlayDownload = document.getElementById("mf-overlay-download");
const overlayMeta = document.getElementById("mf-overlay-meta");
const closeButton = document.getElementById("mf-overlay-close");
const nextButton = document.getElementById("mf-overlay-next");
const prevButton = document.getElementById("mf-overlay-prev");
const thumbs = Array.from(document.querySelectorAll(".mf-thumb"));
let currentIndex = null;

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

  overlayImage.src = full;
  overlayImage.alt = caption;
  overlayDownload.href = full;
  overlayCaption.textContent = caption;
  overlayMeta.textContent = formatExif(button);
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
  currentIndex = null;
  setOverlayHash(null);
}

function moveOverlay(step) {
  if (currentIndex === null || !overlay.classList.contains("open")) {
    return;
  }

  const next = (currentIndex + step + thumbs.length) % thumbs.length;
  openOverlayAt(next);
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

thumbs.forEach((button, index) => {
  button.addEventListener("click", (event) => {
    event.preventDefault();
    openOverlayAt(index);
  });
});

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
openOverlayFromHash();
