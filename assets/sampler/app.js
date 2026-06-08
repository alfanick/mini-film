const overlay = document.getElementById("overlay");
const overlayImage = document.getElementById("overlay-image");
const overlayCaption = document.getElementById("overlay-caption");
const collapsedBranches = new Set(JSON.parse(localStorage.getItem("mini-film-collapsed-branches") || "[]"));
const HOLD_DELAY_MS = 280;
const HOLD_LONG_DELAY_MS = 240;

function suppressContextMenu(event) {
  event.preventDefault();
}

function storeCollapsedBranches() {
  localStorage.setItem("mini-film-collapsed-branches", JSON.stringify([...collapsedBranches]));
}

function setBranchCollapsed(branch, collapsed) {
  const key = branch.dataset.branchKey;
  const toggle = branch.querySelector(":scope > .branch-title .branch-toggle");
  branch.classList.toggle("collapsed", collapsed);
  if (toggle) {
    toggle.setAttribute("aria-expanded", String(!collapsed));
  }
  if (collapsed) {
    collapsedBranches.add(key);
  } else {
    collapsedBranches.delete(key);
  }
}

function openOverlayImage(processedSource, originalSource, title) {
  overlayImage.dataset.processed = processedSource;
  overlayImage.dataset.original = originalSource || processedSource;
  overlayImage.src = processedSource;
  overlayImage.alt = title;
  overlayCaption.textContent = title;
  overlay.classList.add("open");
  overlay.setAttribute("aria-hidden", "false");
}

function openProcessedOverlay(button) {
  openOverlayImage(button.dataset.full, button.dataset.original, button.dataset.title);
}

function openOriginalOverlay(button) {
  openOverlayImage(button.dataset.original || button.dataset.full, button.dataset.original || button.dataset.full, button.dataset.title);
}

document.querySelectorAll(".branch").forEach((branch) => {
  const key = branch.dataset.branchKey;
  const toggle = branch.querySelector(":scope > .branch-title .branch-toggle");
  if (collapsedBranches.has(key)) {
    setBranchCollapsed(branch, true);
  }
  if (toggle) {
    toggle.addEventListener("click", () => {
      setBranchCollapsed(branch, !branch.classList.contains("collapsed"));
      storeCollapsedBranches();
    });
  }
});

document.querySelectorAll(".thumb-button").forEach((button) => {
  button.addEventListener("contextmenu", suppressContextMenu);
  const image = button.querySelector("img");
  if (image) {
    image.addEventListener("contextmenu", suppressContextMenu);
  }
});
overlayImage.addEventListener("contextmenu", suppressContextMenu);

function closeOverlay() {
  overlay.classList.remove("open");
  overlay.setAttribute("aria-hidden", "true");
  overlayImage.removeAttribute("src");
  overlayImage.removeAttribute("data-processed");
  overlayImage.removeAttribute("data-original");
}

document.querySelectorAll(".thumb-button").forEach((button) => {
  let holdTimer = null;
  let held = false;

  const clearHold = () => {
    if (holdTimer !== null) {
      clearTimeout(holdTimer);
      holdTimer = null;
    }
  };

  const onPointerDown = () => {
    held = false;
    clearHold();
    holdTimer = window.setTimeout(() => {
      held = true;
      openOriginalOverlay(button);
    }, HOLD_DELAY_MS);
  };

  const onPointerRelease = () => {
    clearHold();
    if (held && overlay.getAttribute("aria-hidden") === "false") {
      openProcessedOverlay(button);
    }
    held = false;
  };

  button.addEventListener("pointerdown", onPointerDown);
  button.addEventListener("pointerup", onPointerRelease);
  button.addEventListener("pointerleave", clearHold);
  button.addEventListener("pointercancel", clearHold);
  button.addEventListener("click", (event) => {
    if (held) {
      event.preventDefault();
      return;
    }
    openProcessedOverlay(button);
  });
});

let overlayHoldTimer = null;
let overlayHeld = false;

const clearOverlayHold = () => {
  if (overlayHoldTimer !== null) {
    clearTimeout(overlayHoldTimer);
    overlayHoldTimer = null;
  }
};

const restoreOverlayToProcessed = () => {
  const original = overlayImage.dataset.original;
  const processed = overlayImage.dataset.processed;
  if (original !== processed) {
    overlayImage.src = processed || original;
  }
};

const onOverlayPointerDown = () => {
  overlayHeld = false;
  clearOverlayHold();
  const original = overlayImage.dataset.original;
  const processed = overlayImage.dataset.processed;
  if (original === undefined || original === null || original === processed) {
    return;
  }
  overlayHoldTimer = window.setTimeout(() => {
    overlayHeld = true;
    overlayImage.src = original;
  }, HOLD_LONG_DELAY_MS);
};

const onOverlayPointerRelease = () => {
  const wasHeld = overlayHeld;
  clearOverlayHold();
  overlayHeld = false;
  if (wasHeld) {
    restoreOverlayToProcessed();
  }
};

overlayImage.addEventListener("pointerdown", onOverlayPointerDown);
overlayImage.addEventListener("pointerup", onOverlayPointerRelease);
overlayImage.addEventListener("pointerleave", () => {
  clearOverlayHold();
  if (overlayHeld) {
    restoreOverlayToProcessed();
  }
  overlayHeld = false;
});
overlayImage.addEventListener("pointercancel", () => {
  clearOverlayHold();
  if (overlayHeld) {
    restoreOverlayToProcessed();
  }
  overlayHeld = false;
});

overlay.addEventListener("click", (event) => {
  if (event.target === overlay || event.target.classList.contains("overlay-close")) {
    closeOverlay();
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    closeOverlay();
  }
});
