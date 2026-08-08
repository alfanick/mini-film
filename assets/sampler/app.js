const overlay = document.getElementById("overlay");
const overlayImage = document.getElementById("overlay-image");
const overlayCaption = document.getElementById("overlay-caption");
const collapsedBranches = new Set(JSON.parse(localStorage.getItem("mini-film-collapsed-branches") || "[]"));
const HOLD_DELAY_MS = 280;
const HOLD_LONG_DELAY_MS = 240;
const POST_HOLD_CLICK_DELAY_MS = 600;

let overlayHoldTimer = null;
let overlayHeld = false;
let overlayPointerId = null;
let suppressNextOverlayClick = false;
let suppressOverlayClickTimer = null;
let restoreFocusTo = null;

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

function comparisonAvailable() {
  const profile = overlayImage.dataset.profile;
  const diffusion = overlayImage.dataset.diffusion;
  return Boolean(profile && diffusion && diffusion !== profile);
}

function overlayMode() {
  return overlayImage.dataset.mode === "diffusion" && comparisonAvailable() ? "diffusion" : "profile";
}

function overlaySourceForMode(mode) {
  if (mode === "diffusion" && comparisonAvailable()) {
    return overlayImage.dataset.diffusion;
  }
  return overlayImage.dataset.profile;
}

function setOverlayCaption(state) {
  const title = overlayImage.dataset.title || "";
  const labels = {
    profile: "Profile only",
    diffusion: "Diffusion",
    original: "Neutral original",
  };
  const caption = title ? `${title} - ${labels[state]}` : labels[state];
  overlayImage.alt = caption;
  overlayCaption.textContent = caption;
}

function setOverlayMode(mode) {
  const nextMode = mode === "diffusion" && comparisonAvailable() ? "diffusion" : "profile";
  const source = overlaySourceForMode(nextMode);
  overlayImage.dataset.mode = nextMode;
  overlayImage.classList.remove("showing-original");
  if (source) {
    overlayImage.src = source;
  }
  overlayImage.setAttribute("aria-pressed", String(nextMode === "diffusion"));
  overlayImage.setAttribute("aria-disabled", String(!comparisonAvailable()));
  overlayImage.setAttribute(
    "aria-label",
    comparisonAvailable()
      ? `${overlayImage.dataset.title}, ${nextMode === "diffusion" ? "diffusion" : "profile only"}. Activate to show ${nextMode === "diffusion" ? "profile only" : "diffusion"}.`
      : `${overlayImage.dataset.title}, profile only.`,
  );
  setOverlayCaption(nextMode);
}

function showOverlayOriginal() {
  const original = overlayImage.dataset.original;
  const processed = overlaySourceForMode(overlayMode());
  if (!original || original === processed) {
    return false;
  }
  overlayImage.src = original;
  overlayImage.classList.add("showing-original");
  overlayImage.setAttribute(
    "aria-label",
    `${overlayImage.dataset.title}, neutral original. Release to return to ${overlayMode() === "diffusion" ? "diffusion" : "profile only"}.`,
  );
  setOverlayCaption("original");
  return true;
}

function restoreOverlayMode() {
  setOverlayMode(overlayMode());
}

function toggleOverlayMode() {
  if (!comparisonAvailable()) {
    return;
  }
  setOverlayMode(overlayMode() === "profile" ? "diffusion" : "profile");
}

function openProfileOverlay(button) {
  const profile = button.dataset.profile;
  if (!profile) {
    return false;
  }
  restoreFocusTo = document.activeElement;
  overlayImage.dataset.profile = profile;
  overlayImage.dataset.diffusion = button.dataset.diffusion || profile;
  overlayImage.dataset.original = button.dataset.original || profile;
  overlayImage.dataset.title = button.dataset.title || "";
  overlay.classList.add("open");
  overlay.setAttribute("aria-hidden", "false");
  setOverlayMode("profile");
  overlayImage.focus({ preventScroll: true });
  return true;
}

function openOriginalOverlay(button) {
  return openProfileOverlay(button) && showOverlayOriginal();
}

function clearOverlayHold() {
  if (overlayHoldTimer !== null) {
    clearTimeout(overlayHoldTimer);
    overlayHoldTimer = null;
  }
}

function releaseOverlayPointer() {
  if (overlayPointerId !== null && overlayImage.hasPointerCapture(overlayPointerId)) {
    overlayImage.releasePointerCapture(overlayPointerId);
  }
  overlayPointerId = null;
}

function suppressUpcomingOverlayClick() {
  suppressNextOverlayClick = true;
  if (suppressOverlayClickTimer !== null) {
    clearTimeout(suppressOverlayClickTimer);
  }
  suppressOverlayClickTimer = window.setTimeout(() => {
    suppressNextOverlayClick = false;
    suppressOverlayClickTimer = null;
  }, POST_HOLD_CLICK_DELAY_MS);
}

function consumeOverlayClickSuppression() {
  if (!suppressNextOverlayClick) {
    return false;
  }
  suppressNextOverlayClick = false;
  if (suppressOverlayClickTimer !== null) {
    clearTimeout(suppressOverlayClickTimer);
    suppressOverlayClickTimer = null;
  }
  return true;
}

function finishOverlayHold() {
  const wasHeld = overlayHeld;
  clearOverlayHold();
  releaseOverlayPointer();
  overlayHeld = false;
  if (wasHeld) {
    restoreOverlayMode();
    suppressUpcomingOverlayClick();
  }
}

function closeOverlay() {
  clearOverlayHold();
  releaseOverlayPointer();
  overlayHeld = false;
  overlay.classList.remove("open");
  overlay.setAttribute("aria-hidden", "true");
  overlayImage.removeAttribute("src");
  overlayImage.removeAttribute("data-profile");
  overlayImage.removeAttribute("data-diffusion");
  overlayImage.removeAttribute("data-original");
  overlayImage.removeAttribute("data-title");
  overlayImage.removeAttribute("data-mode");
  overlayImage.classList.remove("showing-original");
  overlayImage.alt = "";
  overlayImage.setAttribute("aria-pressed", "false");
  overlayCaption.textContent = "";
  suppressNextOverlayClick = false;
  if (suppressOverlayClickTimer !== null) {
    clearTimeout(suppressOverlayClickTimer);
    suppressOverlayClickTimer = null;
  }
  if (restoreFocusTo && restoreFocusTo.isConnected) {
    restoreFocusTo.focus({ preventScroll: true });
  }
  restoreFocusTo = null;
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
  let holdTimer = null;
  let held = false;
  let pointerId = null;
  let suppressNextClick = false;
  let suppressClickTimer = null;

  const clearHold = () => {
    if (holdTimer !== null) {
      clearTimeout(holdTimer);
      holdTimer = null;
    }
  };

  const releasePointer = () => {
    if (pointerId !== null && button.hasPointerCapture(pointerId)) {
      button.releasePointerCapture(pointerId);
    }
    pointerId = null;
  };

  const suppressUpcomingClick = () => {
    suppressNextClick = true;
    if (suppressClickTimer !== null) {
      clearTimeout(suppressClickTimer);
    }
    suppressClickTimer = window.setTimeout(() => {
      suppressNextClick = false;
      suppressClickTimer = null;
    }, POST_HOLD_CLICK_DELAY_MS);
  };

  const finishHold = () => {
    const wasHeld = held;
    clearHold();
    releasePointer();
    held = false;
    if (wasHeld) {
      restoreOverlayMode();
      suppressUpcomingClick();
    }
  };

  button.addEventListener("contextmenu", suppressContextMenu);
  const image = button.querySelector("img");
  if (image) {
    image.addEventListener("contextmenu", suppressContextMenu);
  }

  button.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || (event.pointerType === "mouse" && event.button !== 0)) {
      return;
    }
    held = false;
    clearHold();
    pointerId = event.pointerId;
    button.setPointerCapture(pointerId);
    holdTimer = window.setTimeout(() => {
      held = openOriginalOverlay(button);
    }, HOLD_DELAY_MS);
  });
  button.addEventListener("pointerup", finishHold);
  button.addEventListener("pointerleave", () => {
    if (pointerId !== null && !button.hasPointerCapture(pointerId)) {
      finishHold();
    }
  });
  button.addEventListener("pointercancel", finishHold);
  button.addEventListener("click", (event) => {
    if (suppressNextClick) {
      suppressNextClick = false;
      if (suppressClickTimer !== null) {
        clearTimeout(suppressClickTimer);
        suppressClickTimer = null;
      }
      event.preventDefault();
      return;
    }
    openProfileOverlay(button);
  });
});

overlayImage.addEventListener("contextmenu", suppressContextMenu);
overlayImage.addEventListener("pointerdown", (event) => {
  if (!event.isPrimary || (event.pointerType === "mouse" && event.button !== 0)) {
    return;
  }
  overlayHeld = false;
  clearOverlayHold();
  overlayPointerId = event.pointerId;
  overlayImage.setPointerCapture(overlayPointerId);
  overlayHoldTimer = window.setTimeout(() => {
    overlayHeld = showOverlayOriginal();
  }, HOLD_LONG_DELAY_MS);
});
overlayImage.addEventListener("pointerup", finishOverlayHold);
overlayImage.addEventListener("pointerleave", () => {
  if (overlayPointerId !== null && !overlayImage.hasPointerCapture(overlayPointerId)) {
    finishOverlayHold();
  }
});
overlayImage.addEventListener("pointercancel", finishOverlayHold);
overlayImage.addEventListener("click", (event) => {
  if (consumeOverlayClickSuppression()) {
    event.preventDefault();
    return;
  }
  toggleOverlayMode();
});
overlayImage.addEventListener("keydown", (event) => {
  if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
    event.preventDefault();
    toggleOverlayMode();
  }
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
