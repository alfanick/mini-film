const overlay = document.getElementById("overlay");
const overlayImage = document.getElementById("overlay-image");
const overlayCaption = document.getElementById("overlay-caption");
const collapsedBranches = new Set(JSON.parse(localStorage.getItem("mini-film-collapsed-branches") || "[]"));
const HOLD_DELAY_MS = 280;

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

function openOverlayImage(source, title) {
  overlayImage.src = source;
  overlayImage.alt = title;
  overlayCaption.textContent = title;
  overlay.classList.add("open");
  overlay.setAttribute("aria-hidden", "false");
}

function openProcessedOverlay(button) {
  openOverlayImage(button.dataset.full, button.dataset.title);
}

function openOriginalOverlay(button) {
  openOverlayImage(button.dataset.original || button.dataset.full, button.dataset.title);
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

function closeOverlay() {
  overlay.classList.remove("open");
  overlay.setAttribute("aria-hidden", "true");
  overlayImage.removeAttribute("src");
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
