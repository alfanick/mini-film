const overlay = document.getElementById("overlay");
const overlayImage = document.getElementById("overlay-image");
const overlayCaption = document.getElementById("overlay-caption");
const collapsedBranches = new Set(JSON.parse(localStorage.getItem("mini-film-collapsed-branches") || "[]"));

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
  button.addEventListener("click", () => {
    overlayImage.src = button.dataset.full;
    overlayImage.alt = button.dataset.title;
    overlayCaption.textContent = button.dataset.title;
    overlay.classList.add("open");
    overlay.setAttribute("aria-hidden", "false");
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
