const invoke = window.__TAURI__?.core?.invoke;

const fields = {
  input: document.getElementById("input"),
  output: document.getElementById("output"),
  profilesRoot: document.getElementById("profilesRoot"),
  profiles: document.getElementById("profiles"),
  reviewAddress: document.getElementById("reviewAddress"),
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
};

const form = document.getElementById("wizard");
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
    reviewAddress: fields.reviewAddress.value.trim(),
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
  };
}

async function loadDefaults() {
  if (!invoke) {
    setStatus("Tauri bridge is not available.", true);
    return;
  }

  const defaults = await invoke("app_defaults");
  version.textContent = `mini-film ${defaults.version}`;
  setIfEmpty(fields.profilesRoot, defaults.profilesRoot);
  setIfEmpty(fields.haldDir, defaults.haldDir);
  setIfEmpty(fields.reviewAddress, defaults.reviewAddress);
  setIfEmpty(fields.jobs, String(defaults.jobs));
  setIfEmpty(fields.jpgQuality, String(defaults.jpgQuality));
  setIfEmpty(fields.rawtherapee, defaults.rawtherapee);
  setIfEmpty(fields.convert, defaults.convert);
  setIfEmpty(fields.publishAlbum, defaults.publishAlbum);
  setIfEmpty(fields.colorNoiseIsoThreshold, String(defaults.colorNoiseIsoThreshold));
  fields.progressiveJpeg.checked = Boolean(defaults.progressiveJpeg);
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!invoke) return;

  start.disabled = true;
  setStatus("Validating settings and starting daemon...");
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
