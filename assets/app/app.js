const invoke = window.__TAURI__?.core?.invoke;

const fields = {
  input: document.getElementById("input"),
  output: document.getElementById("output"),
  profilesRoot: document.getElementById("profilesRoot"),
  profiles: document.getElementById("profiles"),
  reviewPort: document.getElementById("reviewPort"),
  allowOthers: document.getElementById("allowOthers"),
  jobs: document.getElementById("jobs"),
  longEdge: document.getElementById("longEdge"),
  jpgQuality: document.getElementById("jpgQuality"),
  gallery: document.getElementById("gallery"),
  publishAlbum: document.getElementById("publishAlbum"),
  rawtherapee: document.getElementById("rawtherapee"),
  convert: document.getElementById("convert"),
  haldDir: document.getElementById("haldDir"),
  nikonWtu: document.getElementById("nikonWtu"),
  colorNoiseIsoThreshold: document.getElementById("colorNoiseIsoThreshold"),
  grainPreset: document.getElementById("grainPreset"),
  progressiveJpeg: document.getElementById("progressiveJpeg"),
  noGrain: document.getElementById("noGrain"),
  lensCorrections: document.getElementById("lensCorrections"),
  codex: document.getElementById("codex"),
  codexTags: document.getElementById("codexTags"),
  codexNote: document.getElementById("codexNote"),
  codexRating: document.getElementById("codexRating"),
  codexBinary: document.getElementById("codexBinary"),
  codexModel: document.getElementById("codexModel"),
  codexTimeout: document.getElementById("codexTimeout"),
};

const form = document.getElementById("wizard");
const chooseProfiles = document.getElementById("chooseProfiles");
const profilePicker = document.getElementById("profilePicker");
const profileTree = document.getElementById("profileTree");
const profileFilter = document.getElementById("profileFilter");
const profilePickerRoot = document.getElementById("profilePickerRoot");
const profilePickerCount = document.getElementById("profilePickerCount");
const closeProfilePicker = document.getElementById("closeProfilePicker");
const clearProfiles = document.getElementById("clearProfiles");
const applyProfiles = document.getElementById("applyProfiles");
const status = document.getElementById("status");
const start = document.getElementById("start");
const version = document.getElementById("version");

function setStatus(message, isError = false) {
  status.textContent = message;
  status.classList.toggle("error", isError);
}

function setIfEmpty(field, value) {
  if (!field || field.value) return;
  field.value = value ?? "";
}

function numericValue(field) {
  const raw = field.value.trim();
  if (!raw) return null;
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}

function requestFromForm() {
  return {
    input: fields.input.value.trim(),
    output: fields.output.value.trim(),
    profilesRoot: fields.profilesRoot.value.trim(),
    profiles: fields.profiles.value
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean),
    reviewPort: numericValue(fields.reviewPort),
    allowOthers: fields.allowOthers.checked,
    jobs: numericValue(fields.jobs),
    longEdge: numericValue(fields.longEdge),
    jpgQuality: numericValue(fields.jpgQuality),
    gallery: fields.gallery.value,
    publishAlbum: fields.publishAlbum.value.trim(),
    rawtherapee: fields.rawtherapee.value.trim(),
    convert: fields.convert.value.trim(),
    haldDir: fields.haldDir.value.trim(),
    nikonWtu: fields.nikonWtu.value.trim(),
    colorNoiseIsoThreshold: numericValue(fields.colorNoiseIsoThreshold),
    grainPreset: fields.grainPreset.value,
    progressiveJpeg: fields.progressiveJpeg.checked,
    noGrain: fields.noGrain.checked,
    lensCorrections: fields.lensCorrections.checked,
    codex: fields.codex.checked,
    codexTags: fields.codexTags.checked,
    codexNote: fields.codexNote.checked,
    codexRating: fields.codexRating.checked,
    codexBinary: fields.codexBinary.value.trim(),
    codexModel: fields.codexModel.value.trim(),
    codexTimeout: numericValue(fields.codexTimeout),
  };
}

async function loadDefaults() {
  if (!invoke) {
    setStatus("Tauri bridge is not available.", true);
    return;
  }

  const defaults = await invoke("app_defaults");
  version.textContent = `mini-film ${defaults.version}`;
  setIfEmpty(fields.input, defaults.input);
  setIfEmpty(fields.output, defaults.output);
  setIfEmpty(fields.profilesRoot, defaults.profilesRoot);
  if (Array.isArray(defaults.profiles) && defaults.profiles.length > 0 && !fields.profiles.value) {
    fields.profiles.value = defaults.profiles.join("\n");
  }
  setIfEmpty(fields.haldDir, defaults.haldDir);
  setIfEmpty(fields.reviewPort, String(defaults.reviewPort));
  setIfEmpty(fields.jobs, String(defaults.jobs));
  if (defaults.longEdge) setIfEmpty(fields.longEdge, String(defaults.longEdge));
  setIfEmpty(fields.jpgQuality, String(defaults.jpgQuality));
  setIfEmpty(fields.gallery, defaults.gallery);
  setIfEmpty(fields.rawtherapee, defaults.rawtherapee);
  setIfEmpty(fields.convert, defaults.convert);
  setIfEmpty(fields.publishAlbum, defaults.publishAlbum);
  setIfEmpty(fields.nikonWtu, defaults.nikonWtu);
  setIfEmpty(fields.colorNoiseIsoThreshold, String(defaults.colorNoiseIsoThreshold));
  setIfEmpty(fields.grainPreset, defaults.grainPreset);
  fields.allowOthers.checked = Boolean(defaults.allowOthers);
  fields.progressiveJpeg.checked = Boolean(defaults.progressiveJpeg);
  fields.noGrain.checked = Boolean(defaults.noGrain);
  fields.lensCorrections.checked = Boolean(defaults.lensCorrections);
  fields.codex.checked = Boolean(defaults.codex);
  fields.codexTags.checked = Boolean(defaults.codexTags);
  fields.codexNote.checked = Boolean(defaults.codexNote);
  fields.codexRating.checked = Boolean(defaults.codexRating);
  setIfEmpty(fields.codexBinary, defaults.codexBinary);
  setIfEmpty(fields.codexModel, defaults.codexModel);
  setIfEmpty(fields.codexTimeout, String(defaults.codexTimeout));
}

function directoryTitle(fieldName) {
  if (fieldName === "input") return "Choose input inbox";
  if (fieldName === "output") return "Choose output folder";
  if (fieldName === "profilesRoot") return "Choose profiles root";
  return "Choose folder";
}

function profileLines() {
  return fields.profiles.value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

document.querySelectorAll("[data-pick-directory]").forEach((button) => {
  button.addEventListener("click", async () => {
    const fieldName = button.dataset.pickDirectory;
    const field = fields[fieldName];
    if (!field || !invoke) return;

    button.disabled = true;
    try {
      const selected = await invoke("pick_directory", {
        title: directoryTitle(fieldName),
        start: field.value.trim() || null,
      });
      if (selected) field.value = selected;
    } catch (error) {
      setStatus(String(error), true);
    } finally {
      button.disabled = false;
    }
  });
});

chooseProfiles.addEventListener("click", async () => {
  if (!invoke) return;

  chooseProfiles.disabled = true;
  setStatus("Loading profile tree...");
  try {
    const tree = await invoke("profile_tree", {
      profilesRoot: fields.profilesRoot.value.trim(),
    });
    openProfilePicker(tree);
    setStatus(`Loaded ${tree.count} profiles.`);
  } catch (error) {
    setStatus(String(error), true);
  } finally {
    chooseProfiles.disabled = false;
  }
});

function openProfilePicker(tree) {
  profilePickerRoot.textContent = tree.root;
  profileFilter.value = "";
  renderProfileTree(tree, new Set(profileLines()));
  profilePicker.hidden = false;
  profileFilter.focus();
}

function closeProfiles() {
  profilePicker.hidden = true;
}

function renderProfileTree(tree, selected) {
  profileTree.replaceChildren();
  const fragment = document.createDocumentFragment();
  for (const node of tree.children ?? []) {
    fragment.appendChild(renderProfileNode(node, selected, 0));
  }
  profileTree.appendChild(fragment);
  updateProfileCount();
}

function renderProfileNode(node, selected, depth) {
  const details = document.createElement("details");
  details.className = "profile-node";
  details.open = depth < 2;

  const summary = document.createElement("summary");
  summary.textContent = node.label;
  details.appendChild(summary);

  const body = document.createElement("div");
  body.className = "profile-node-body";
  for (const profile of node.profiles ?? []) {
    body.appendChild(renderProfileLeaf(profile, selected));
  }
  for (const child of node.children ?? []) {
    body.appendChild(renderProfileNode(child, selected, depth + 1));
  }
  details.appendChild(body);
  return details;
}

function renderProfileLeaf(profile, selected) {
  const row = document.createElement("label");
  row.className = "profile-leaf";
  row.dataset.search = `${profile.name} ${profile.relative}`.toLowerCase();

  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.value = profile.path;
  checkbox.checked = selected.has(profile.path);
  checkbox.addEventListener("change", updateProfileCount);

  const text = document.createElement("span");
  text.textContent = profile.name;

  const relative = document.createElement("small");
  relative.textContent = profile.relative;

  row.append(checkbox, text, relative);
  return row;
}

function updateProfileCount() {
  const selected = profileTree.querySelectorAll(".profile-leaf input:checked").length;
  const total = profileTree.querySelectorAll(".profile-leaf input").length;
  profilePickerCount.textContent = `${selected} selected / ${total} profiles`;
}

function applyProfileFilter() {
  const query = profileFilter.value.trim().toLowerCase();
  for (const leaf of profileTree.querySelectorAll(".profile-leaf")) {
    leaf.hidden = Boolean(query) && !leaf.dataset.search.includes(query);
  }
  for (const node of Array.from(profileTree.querySelectorAll(".profile-node")).reverse()) {
    const hasVisibleLeaf = Boolean(node.querySelector(".profile-leaf:not([hidden])"));
    node.hidden = Boolean(query) && !hasVisibleLeaf;
    if (query && hasVisibleLeaf) node.open = true;
  }
}

closeProfilePicker.addEventListener("click", closeProfiles);

profilePicker.addEventListener("click", (event) => {
  if (event.target === profilePicker) closeProfiles();
});

profileFilter.addEventListener("input", applyProfileFilter);

clearProfiles.addEventListener("click", () => {
  for (const checkbox of profileTree.querySelectorAll(".profile-leaf input")) {
    checkbox.checked = false;
  }
  updateProfileCount();
});

applyProfiles.addEventListener("click", () => {
  const selected = Array.from(profileTree.querySelectorAll(".profile-leaf input:checked")).map(
    (checkbox) => checkbox.value,
  );
  fields.profiles.value = selected.join("\n");
  closeProfiles();
});

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!invoke) return;

  start.disabled = true;
  setStatus("Validating settings, starting daemon, and waiting for review server...");
  try {
    const response = await invoke("start_app_daemon", { request: requestFromForm() });
    setStatus(`Daemon started. Opening ${response.reviewUrl}`);
    window.location.href = response.reviewUrl;
  } catch (error) {
    setStatus(String(error), true);
    start.disabled = false;
  }
});

loadDefaults().catch((error) => setStatus(String(error), true));
