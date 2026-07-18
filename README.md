# mini-film

`mini-film` is a complete photo-to-review-to-publish workflow for photographers
who want film-emulation output without opening Lightroom. It watches an inbox of
RAW, JPEG, and HEIC files, develops images through RawTherapee when profiles are
configured, applies user-supplied film profiles and grain, serves a live review UI,
records ratings/tags/notes, and publishes the final selection with metadata
preserved.

The fastest way in is the desktop app:

```sh
mini-film
```

With no command, mini-film opens the native app wizard for the usual
daemon/review settings, profile selection, Nikon WTU ingest, lens corrections,
output folders, and publishing defaults. The app remembers the last successful
setup, starts the same daemon pipeline used by the CLI, waits for the embedded
review server, and then opens the review UI in a webview. Terminal output is
still kept when launched from a shell, so long-running processing stays
inspectable. `mini-film app` remains available as the explicit command form.

For scripted use, every part of the workflow is still available as CLI commands:
`apply`, `batch`, `daemon`, `sampler`, `pp3`, `info`, `nikon`, and `update`.

## Screenshots

Main review view:

![mini-film main review view](https://sam.nakarmamana.ch/mini-film-ss-main.png)

Publishing workflow:

![mini-film publishing workflow](https://sam.nakarmamana.ch/mini-film-ss-publish.png)

Available shortcuts:

![mini-film shortcuts overlay](https://sam.nakarmamana.ch/mini-film-ss-shortcuts.png)

## Top Features

- **Single-binary desktop workflow**: `mini-film` provides a Tauri launcher
  with native directory pickers, a structured profile tree, saved options,
  network sharing toggle, Nikon WTU IP input, and system light/dark theme.
- **Live review and publish workflow**: run `daemon` on an inbox folder, open the
  browser UI, review new pictures as they arrive, rate/tag/label them in
  multiple passes, compare profile variants, and publish the final selection
  with live job progress. The review workflow is optimized for fast keyboard
  operation, while still supporting mouse, touch, and tablet use.
- **Mixed RAW and processed-file input**: batch, daemon, review, and publish can
  take RAW files plus existing JPEG/HEIC files. Explicit profiles apply to all
  standalone inputs; without a profile, JPEG/HEIC keeps its existing look and
  skips RawTherapee. Compressed originals remain available for saving.
- **Optional Codex review assist**: let Codex analyze small embedded RAW
  previews after rendering finishes and fill tags, notes, or initial ratings
  while the live review UI shows the same queued/processing status indicators.
- **Film emulation from user-supplied profiles**: apply XMP emulation presets,
  Hald CLUT PNGs, or RawTherapee `.pp3` files; convert Adobe Camera Raw /
  Lightroom `crs:RGBTable` profile XMPs into cached 16-bit Hald PNGs.
- **Nikon WTU ingest**: pair with Nikon Connect-to-PC / Wireless Transmitter
  Utility mode over built-in camera Wi-Fi and feed transferred RAWs directly into
  the daemon inbox.
- **Batch, sampler, and gallery output**: process whole folders, render profile
  sampler sheets, and generate modern static HTML galleries.
- **RAW pipeline extras**: RawTherapee auto-matched camera tone curves,
  deterministic film grain, optional high-ISO color denoise, optional
  RawTherapee lens corrections, metadata copyback with `exiftool`, 8-bit JPEG
  output, and 16-bit Zip-compressed TIFF output.

## Profile Library

mini-film does not include film profiles or emulations. It works with XMP, Hald,
and PP3 profiles already present on disk, including profiles created by the user
or obtained from commercial film-emulation products.

For XMP preset libraries, point `--profiles-root` at a directory with:

```text
profiles/     # internal RGBTable profile XMPs
emulations/   # user-facing emulation preset XMPs
```

## Quick Start

Apply one profile to one RAW, JPEG, or HEIC image:

```sh
mini-film apply input.RAW \
  --profile 'Classic Film' \
  --profiles-root /path/to/profile-library \
  --output output.jpg
```

Batch-process a folder, accepting RAW plus JPEG/HEIC inputs by default:

```sh
mini-film batch /path/to/photos /path/to/output \
  --profile 'Classic Film' \
  --profiles-root /path/to/profile-library \
  --long-edge 3840 \
  --progressive-jpeg
```

Use `--input-raw-only` or `--input-jpg-only` when a mixed input folder should be
filtered before processing. JPEG and HEIC are treated as the same processed-file
group for these filters.

In the default mixed mode, a JPEG/HEIC with the same normalized stem in the same
folder as a RAW is treated as that RAW's straight-out-of-camera sidecar instead
of a separate picture. A trailing dash-number suffix is ignored for this match,
so `DSC_8313-1.jpg` pairs with `DSC_8313-2.dng`. The review UI shows it as an
unchecked `straight out of camera` profile thumbnail; if selected for publish,
its output uses the `-sooc` suffix. If the JPEG/HEIC reaches the review state
before the RAW, mini-film folds it into the RAW as soon as the RAW appears, and
performs the same cleanup when an existing SQLite review database is reopened.

Image files that are symlinks inside the input folder are treated like normal
input files, including review and publish rerenders.

Run the live review workflow:

```sh
mini-film daemon /path/to/inbox /path/to/output \
  --profile 'Classic Film' \
  --profile 'Soft Fade' \
  --profiles-root /path/to/profile-library \
  --review-address 0.0.0.0:8090
```

Then open `http://localhost:8090`, review pictures as they are processed, and use
the publish wizard to export the final album.

Run the desktop wizard instead of writing the daemon command manually:

```sh
mini-film
```

The app wizard defaults to `~/Pictures/Scratch/Inbox` for input,
`~/Pictures/mini-film` for output, and `MINI_FILM_PROFILES_ROOT` or the selected
profiles root for profile lookup. It remembers the last successful setup in
`~/.cache/mini-film/app-settings.json`.

Use Nikon WTU ingest with the same daemon:

```sh
mini-film daemon /path/to/inbox /path/to/output \
  --profile 'Classic Film' \
  --profiles-root /path/to/profile-library \
  --review-address 0.0.0.0:8090 \
  --nikon-wtu 192.168.1.50
```

## Build

```sh
cargo build --release
```

The default build includes the Tauri desktop app so `cargo run --release` opens
the app from the normal binary. On Linux this requires the WebKitGTK development
packages used by Tauri. On Debian/Ubuntu/Linux Mint:

```sh
sudo apt-get install \
  libwebkit2gtk-4.1-dev \
  libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

To build the command-line binary without desktop GUI dependencies:

```sh
cargo build --release --no-default-features
```

GitHub releases publish both CLI artifacts and GUI artifacts. GUI artifact names
end in `-gui` before the platform extension, and GUI builds update from matching
`-gui` release assets.

Required external dependencies at startup for image-generation commands:

- `rawtherapee-cli`
- `convert` (ImageMagick/GraphicsMagick)
- `exiftool`
- `codex` only when daemon review analysis is enabled with `--codex`

The `update` command also requires `curl` for downloading the Lensfun database.

## Coverage

Generate LCOV coverage locally (requires `cargo-llvm-cov`):

```sh
cargo install --locked cargo-llvm-cov
./scripts/coverage.sh
```

The script writes:

```sh
target/coverage/lcov.info
```

CI runs the same coverage step on every push/PR and uploads `coverage-lcov` as an artifact.

### Update

Use `update` to manually refresh the `mini-film` executable from
`https://github.com/alfanick/mini-film/releases` and rebuild/update the local
Lensfun profile database in `~/.cache/mini-film/lensfun`.

The command also mirrors Lensfun DB files into the platform default location
used by RawTherapee.

```sh
mini-film update
```

Update behavior is not automatic. Run it explicitly when a new release is
expected or before long sessions. The binary refresh still requires this crate to
be built with `--features github-update`; the Lensfun database refresh does not.

## Convert XMP Profiles To Hald

Convert one profile XMP:

```sh
cargo run --release -- hald \
  '/path/to/profile-library/profiles/Kodak Portra 400 normalised profile.xmp' \
  -o '../hald/Kodak Portra 400 normalised.hald.png' \
  --overwrite
```

Convert all profile XMPs under the parent directory:

```sh
cargo run --release -- hald /path/to/profile-library/profiles --overwrite
```

Ordinary emulation preset XMPs usually only reference a profile UUID and do not
contain the table payload; `hald` skips those in directory mode.

When `-o/--output` is omitted, `hald` writes generated Hald PNGs under
`$HOME/.cache/mini-film/hald`. Hald PNGs contain only the decoded RGBTable
lookup. Profile XMPs that include extra Camera Raw settings print
`adjustments=pp3` or `sharpening=pp3`; those settings are handled through
generated RawTherapee profiles during `apply`, `batch`, and `sampler`.

## Apply A Complete Film Recipe

Use a Lightroom emulation XMP that references an internal profile and defines grain:

```sh
cargo run --release -- apply input.RAW \
  --profile '/path/to/profile-library/emulations/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root /path/to/profile-library \
  -o output.tif
```

If `--profile` is an emulation name, set `--profiles-root` to a folder that
contains `emulations/` and `profiles/`:

```sh
cargo run -- apply \
  --output /home/alfanick/test.jpg \
  --profile 'Agfa Scala 200 + grainy' \
  --profiles-root /path/to/profile-library \
  /home/alfanick/Pictures/Lightroom/2026/05/03/DSC_1812-10.dng
```

`output.tif` / `output.tiff` is exported as 16-bit Zip-compressed TIFF.

```sh
cargo run --release -- apply input.RAW \
  --profile '/path/to/profile-library/emulations/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root /path/to/profile-library \
  -o output.jpg
```

`output.jpg` / `output.jpeg` is exported as 8-bit JPEG.

Use a human-edited RawTherapee profile directly:

```sh
cargo run --release -- apply input.RAW \
  --profile edited-profile.pp3 \
  -o output.jpg
```

When `--profile` points at a `.pp3`, mini-film passes that PP3 directly to RawTherapee. If the PP3 contains a `[Film Simulation]` section, RawTherapee applies its referenced Hald during RAW development. PP3-only profiles do not carry mini-film grain metadata.

Output metadata behavior for `apply`, `batch`, `daemon`, and `sampler`:

- source EXIF/IPTC fields are copied from the input RAW to the output file using
  `exiftool` (unless `--strip-metadata` is set)
- generated outputs are annotated with Adobe-style XMP Camera Raw settings
  (`XMP-crs`) for resolved mini-film profile adjustments, sharpening, HSL,
  calibration, tone-curve, and grain values where mini-film has real structured
  data for the render
- the XMP packet also records mini-film as the creator/converter and adds a
  high-level `xmpMM` history entry with the raw file, profile, linked profile,
  Hald or PP3 source, grain state, grain engine, and grain seed
- an EXIF comment is written as  
  `mini-film <version> usage=<command> profile=<profile-or-emulation>`
  or `profile=none` when no profile was configured

For direct JPEG/HEIC inputs without a profile, source metadata is copied from the
compressed source and mini-film only adds its basic version/comment marker and
review/publish metadata. Profiled JPEG/HEIC outputs receive the same resolved
profile, grain, and edit-history metadata as profiled RAW outputs.

## Batch Apply

Process every supported RAW file (`.dng`, `.nef`, `.cr2`, `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`, etc.) plus JPEG/HEIC files (`.jpg`, `.jpeg`, `.heic`, `.heif`) under an input directory and write JPGs or 16-bit TIFFs under an output directory:

```sh
cargo run --release -- batch \
  /home/alfanick/Pictures/Lightroom/2026/05/03 \
  /home/alfanick/batch-output \
  --profile 'Agfa Scala 200 + grainy' \
  --profiles-root /path/to/profile-library \
  --output-format jpg \
  --jobs 8
```

The output directory is created if it does not exist. Nested input folders are
preserved, and each input uses the same relative path with a `.jpg` extension by
default. Use `--output-format tiff` to write `.tif` files through the 16-bit
Zip-compressed TIFF path. With `--profile`, standalone JPEG/HEIC inputs use the
same PP3, Hald, grain, retouch, and black-and-white profile pipeline as RAW.
JPEG goes directly to RawTherapee; HEIC is first auto-oriented into a 16-bit
Zip-compressed TIFF. In daemon review, a straight-out-of-camera rendition also
appears as a profile and is excluded from publish until selected. Without
`--profile`, compressed inputs retain the direct resize/crop/rotation and
metadata-copy path. Camera tone matching, automatic ISO denoise, and lens
corrections remain RAW-only. During folder discovery, a JPEG/HEIC with a matching
RAW is treated as its straight-out-of-camera sidecar; only the RAW receives
profile renders.

By default, `batch` processes half of the detected CPU threads at once. On a 16-thread CPU that means 8 files in parallel. Override it with `--jobs N` when tuning for a different machine, output format, or memory budget.

`batch` shows two progress bars:

- total batch progress across files
- current file progress across RAW decode/Hald/grain or compressed-file export
  steps

### Batch Gallery

Pass `--gallery` to generate a gallery HTML from all successful batch outputs:

```sh
mini-film batch \
  /home/alfanick/Pictures/Lightroom/2026/05/03 \
  /home/alfanick/batch-output \
  --profile 'Agfa Scala 200 + grainy' \
  --gallery modern \
  --gallery-columns 4 \
  --gallery-thumbnail-long-edge 1024
```

`--gallery` accepts one of `modern`, `soft`, `compact`, `hero`, `phone`, or `all`.

Template intent:

- `modern`: masonry-like variable-cards layout with generous spacing and hero-like
  first-look cadence
- `soft`: editorial light look with warm spacing and caption emphasis
- `compact`: dense compact grid for quick scanning
- `hero`: story-focused layout with a large leading tile and balanced follow-ups
- `phone`: iOS/iPadOS photos style, dense square tiles
- `all`: render all five templates

Batch gallery options are folder-friendly and reuse the existing batch output tree
(including subdirectories). For a single template, the gallery is written as
`<output>/index.html` and uses `<output>/thumbnails/`.

`--gallery all` renders all five gallery templates into:

- `<output>/modern/index.html`
- `<output>/soft/index.html`
- `<output>/compact/index.html`
- `<output>/hero/index.html`
- `<output>/phone/index.html`

The template run reuses one shared thumbnail cache at
`<output>/.mini-film-gallery-thumbnails/` so switching layouts does not reprocess
or regenerate thumbnails.

## Daemon

Run a long-lived watcher that applies optional profiles whenever new files
arrive in an input folder. If no `--profile` is provided, each RAW is developed
once with RawTherapee defaults and JPEG/HEIC inputs are converted directly.
When profiles are provided, each standalone RAW, JPEG, or HEIC gets one output
per profile. Prepared HEIC TIFFs are reused from
`<output>/.mini-film-profile-inputs/heic-v1/` across profile and retouch renders.
If a matching RAW exists, its JPEG/HEIC sidecar is never profiled separately.

```sh
mini-film daemon \
  /home/alfanick/Pictures/Lightroom/inbox \
  /home/alfanick/Pictures/mini-film-output \
  --profile 'Agfa Scala 200 + grainy' \
  --profile 'Portra 400' \
  --profiles-root /path/to/profile-library \
  --debounce-seconds 1 \
  --jobs 8 \
  --output-format jpg
```

The command validates configured profiles on startup, so mistyped `--profile`
values fail immediately. It watches the input directory recursively, waits for the file to
be reported as completed by the watcher (or a short fallback window when that
signal is not available), and writes each
result into:

```text
<raw relative structure>/<profile stem>/<raw stem>.<ext>
```

Without `--profile`, daemon output uses the raw-relative structure directly:

```text
<raw relative structure>/<raw stem>.<ext>
```

For example, `/in/2026/05/03/DSC_1864-14.dng` with profiles `Foil` and `Classic`
becomes:

- `/out/2026/05/03/Foil/DSC_1864-14.jpg`
- `/out/2026/05/03/Classic/DSC_1864-14.jpg`

If profile resolution fails for any selector, startup stops with a clear error.

`daemon` processes input files in parallel and defaults to half the available
CPU threads unless `--jobs` is set.

### Live Review

Add `--review-address` to expose a browser-based review UI while the daemon is
running:

```sh
mini-film daemon \
  /home/alfanick/Pictures/Lightroom/inbox \
  /home/alfanick/Pictures/mini-film-output \
  --profile 'Classic Film' \
  --profile 'Soft Fade' \
  --profiles-root /path/to/profile-library \
  --review-address 0.0.0.0:8090 \
  --gallery modern
```

The review server assets are compiled into the binary, so a release executable
does not need HTML/CSS/JS files next to it. The UI is live: the daemon records
new RAW and JPEG/HEIC files immediately, extracts an embedded camera preview when
available, then updates the browser over server-sent events as each render moves
from queued to processing to done. When no profiles are configured, the review UI
shows the developed RawTherapee-default output without a profile rail and direct
JPEG/HEIC inputs hide profile and retouch controls. With explicit profiles,
standalone JPEG/HEIC inputs expose the same profile rail, retouch controls,
black-and-white filters, and publish selection as RAW. Their camera rendition
appears in the rail as `straight out of camera`, unchecked for publish by
default. For
standalone JPEG/HEIC, ordered background thumbnail and preview pipelines start
at discovery time and run concurrently with full-output export. Each converter
uses half of the available CPU threads; one thumbnail worker and two preview
workers keep review throughput high without overwhelming metadata discovery.
Jobs follow capture time and then file name, and each derivative becomes
available to the browser as soon as its cache file lands. The sidebar uses
progressive 512-pixel quality-55 thumbnails; the main viewer uses progressive
2048-pixel quality-82 previews. In JPEG-only reviews it preloads the next three
previews at viewport sizes up to 2048 pixels and the next three originals above
that threshold. Both tiers are cached under
`<output>/.mini-film-review-previews/compressed-v1/`. Direct compressed images
request the original input only when the loupe activates or the browser viewport
is larger than 2048 pixels; profiled compressed images use the selected profile
render for the main viewer and loupe. Holding the primary mouse button or a touch
shows the nearby loupe until release. On desktop, double-clicking the picture
toggles a full-view zoom that follows the mouse cursor; double-click again or
press Escape to leave it. In the desktop and tablet layouts, direct
JPEG/HEIC inputs hide the unavailable Retouch section. RAW inputs paired with a
straight-out-of-camera JPEG/HEIC keep that section in place and disable it while
the straight-out-of-camera profile is selected, so switching profiles does not
shift the page layout. In the phone layout, direct JPEG/HEIC inputs hide the
unavailable Retouch action. All JPEG/HEIC inputs expose a Save Photo action for
the untouched source file. On
secure origins it opens the native file-share sheet, including iOS's Save Image
action; on plain LAN HTTP it opens the correctly typed original image for
Safari's image-save actions. The first
`--profile` is the default selected look for RAW and standalone compressed files
when profiles are configured. All configured profile variants are selected for publish by default;
use the checkbox on each profile thumbnail to exclude or re-include a variant
while reviewing.
Hover the filename in the review panel to see the source file size, pixel
dimensions, and megapixel count rounded to one decimal place.
When the source metadata contains a shutter count, hover the camera name in the
same panel to see `Shutter count: N`. Hover the focal length to see the lens
name when it is available. Hover the shutter speed to see the shutter mode and,
when enabled, silent photography. Hover the PSAM mode to see the release mode.
When Nikon Auto ISO is active, hover ISO to see `Auto ISO <= ISO N` with the
configured upper limit.

Multiple browsers can use the review page at the same time. The current picture
and minimum rating filter are server-owned shared state, so every browser shows
the same review position. Navigation, filter changes, keyboard ratings, and
rating-button clicks update that shared state and are replicated to every
connected browser through the server-sent event stream. At the end of a pass,
the shared filter moves to the next rating level.

Review data and the shared browser position are persisted in
`<output>/mini-film-review.sqlite` as normalized relational rows; the active
database does not keep an opaque JSON copy of the review store. Databases from
older mini-film releases are migrated automatically in one SQLite transaction
and the old snapshot is removed only after the relational data reconstructs the
complete review state exactly and passes SQLite integrity checks. Existing
`<output>/mini-film-review.json` state files use the same verified relational
migration and are renamed only after the new database is installed. A
human-readable audit trail of review, render, and publish state changes is
appended to `<output>/history.txt`. Together those files make it possible to
restart the daemon, reopen the browser, and continue the multi-pass culling flow
where it stopped. A typical pass is to rate everything `0` or `1`, filter to
`>= 1`, rate the survivors higher, and repeat until the final subset is ready.

The review workflow is optimized for keyboard operation. Press `?` in the
browser UI to show the shortcuts overlay. `§` rates `0` and advances, `1`-`5`
rates and advances, arrow up/down changes the rating and advances, left/right
navigates without rating, `h` toggles the luma/RGB histogram in the workspace's
top-left corner on desktop, PageUp/PageDown cycles the profile preview for the
current picture, Space includes or skips the
selected profile for publish, and Escape leaves the tags or notes field without
advancing. Double-click or
double-tap a profile thumbnail to publish only that profile and exclude the
others. `6`, `7`, `8`, `9`, and `0` toggle red, yellow, green, blue, and purple
labels without advancing; `r`, `y`, `g`, `b`, and `p` provide the same mnemonic
label toggles. `c` copies the current retouch slider adjustments, and `v` pastes
them onto another picture. While crop mode is open, `r` rotates the selected
crop ratio instead of toggling the red label.

The review UI stores rating, label, tags, notes, active preview profile, and the
set of profile variants selected for publish. RAW and explicitly profiled
JPEG/HEIC inputs support per-picture
retouch controls for exposure, highlights, shadows, whites, blacks, as-shot
color temperature, clarity, rotation, and crop. The temperature control is
centered on the camera's Kelvin value, and hovering `Temperature` shows the
camera white-balance mode when available. `Tint` is centered on the camera's
green/magenta white-balance fine-tune value. Moving either white-balance control
generates an effective custom RawTherapee white balance from those as-shot
values. Exposure, highlights, shadows, whites, and blacks display the combined
source-profile and emulation values for the selected profile plus any manual
per-picture adjustment. Manual edits remain stored as deltas, so changing the
selected profile updates those five slider baselines without applying profile
values twice. Direct JPEG/HEIC inputs without
a profile do not expose retouch controls. The browser applies a fast draft preview while
edits are being made, then queues a high-quality mini-film render and swaps in
the finished output when it is ready. Crop and rotation are persisted with the
review state and are used by publish rerenders. Reopening crop mode shows an
uncropped camera preview or original SOOC image, with the finished crop aligned
over it, so an existing crop can be expanded or repositioned without losing the
discarded area. Crop ratios can be locked to the auto-oriented original frame,
`4:3`, `5:4`, `A3/A4`, `1:1`, `16:10`, `21:9`, or `3:1`; press `r` in crop mode
to swap the selected ratio's orientation. The A3/A4 preset uses the shared ISO
A-series `sqrt(2):1` paper ratio. The editable frame is constrained to the
largest black-corner-free area after rotation, so neither crop handles nor the
published result can include rotation triangles. Profile variants whose combined
source and emulation saturation is effectively black-and-white show `None`, `Y`,
`O`, `R`, and `G` filter controls beside the selected profile name. Those
black-and-white filter choices are stored per picture/profile variant and are
used by review and publish rerenders. Generated retouch and black-and-white
filter variants are cached as hidden sibling outputs so returning to an already
rendered variant swaps immediately.
Click the selected profile name to open the film simulation info overlay; it
shows profile identity, camera Active D-Lighting metadata for the current RAW,
and a compact summary of generated or direct PP3 adjustment sections. Its
collapsible `Complete PP3` section shows and downloads every PP3 layer applied
to that picture/profile in RawTherapee command-line order, including generated
retouch, black-and-white filter, ISO denoise, lens correction, and auto-matched
curve adjustments when active.

Optional Codex analysis can run after each image has a review preview and its
configured renders have finished. RAW inputs use the embedded camera preview;
JPEG/HEIC inputs get a temporary 640-pixel analysis preview instead of sending
the full-resolution output. Two Codex analyses run in parallel:

```sh
mini-film daemon \
  /path/to/inbox \
  /path/to/output \
  --profile 'Classic Film' \
  --profiles-root /path/to/profile-library \
  --review-address 0.0.0.0:8090 \
  --codex tags,note
```

`--codex` without a value defaults to tags only. Explicit values are
comma-separated: `tags`, `note`, `rating`, or `all`. Tags are normalized into the
same tag field used by manual review. Notes are one-sentence descriptions.
Ratings use a conservative `0` to `3` scale where `0` means technically
unusable, `1` technically correct, `2` interesting, and `3` truly good. Manual
edits always win: once a user changes tags, notes, or rating, later Codex
results do not overwrite that field. Camera ratings imported from RAW metadata,
including Nikon in-camera star ratings exposed by ExifTool as `Rating` or
Nikon/XMP rating tags, are protected from Codex rating updates too. While
analysis is queued or running, the existing review sidebar status dot/text and
the main profile status line show the Codex state.

The default model is `gpt-5.4-mini`, and review analysis runs it with isolated
low reasoning so unrelated user Codex settings do not make tagging slower or
incompatible. Override the model or binary path with:

```sh
--codex-model gpt-5.4-mini
--codex-binary codex
--codex-timeout 45
```

Publishing opens a wizard that acts as a browser frontend for a spawned
`mini-film review-publish` job. The wizard can filter by rating, color label,
and tags; choose a relative album folder inside the daemon output directory;
select JPG/TIFF output, original size or resize options, JPEG
quality/subsampling/progressive mode, and an optional gallery template.
Publish jobs run in parallel with the daemon job count, which defaults to half
the available CPU threads, and stream live progress back to every open review
browser.

The default publish settings match the daemon render settings, so publishing
uses hardlinks to the already-reviewed outputs when possible and falls back to
symlinks if hardlinks are not available. If output settings differ, mini-film
rerenders the selected pictures from the original RAW, JPEG, or HEIC inputs
through the normal `apply` pipeline before building the gallery.

Published outputs are flat under the chosen album folder:

```text
<output>/<album>/<raw stem>.<ext>
<output>/<album>/<raw stem>-<profile stem>.<ext>
```

With no daemon `--profile`, published files use `<raw stem>.<ext>`. Otherwise,
the first daemon `--profile` is treated as the default profile and does not add a
profile suffix. Additional selected profiles add `-<profile stem>`. If a gallery
template is chosen in the wizard, publish renders an `index.html` gallery in the
album folder using the same templates as batch. Supported templates are
`modern`, `soft`, `compact`, `hero`, `phone`, and `all`. When publishing
finishes, the review wizard shows both `Open gallery` and `Download gallery`;
the download is one portable ZIP containing the HTML, gallery assets,
thumbnails, and full published images.

Published JPGs are annotated with review metadata through `exiftool`: rating,
label, tags, published profile, notes, and a mini-film version/comment marker.
The source RAW metadata and mini-film XMP edit metadata copied during normal
output generation are preserved unless `--strip-metadata` is used.

The same publish operation can be run directly from the CLI:

```sh
mini-film review-publish \
  --state /home/alfanick/Pictures/mini-film-output/mini-film-review.sqlite \
  --input-root /home/alfanick/Pictures/Lightroom/inbox \
  --output-root /home/alfanick/Pictures/mini-film-output \
  --album published/final \
  --min-rating 3 \
  --label green \
  --gallery modern
```

Daemon can also ingest files from a Nikon camera configured for
Connect-to-PC / Wireless Transmitter Utility style transfer. Start the camera's
connection wizard, make sure the computer can reach the camera address, then
point mini-film at that address. mini-film drives the Nikon pairing/auth step,
downloads RAWs into the watched inbox, and processes them with the normal daemon
queue:

```sh
mini-film daemon \
  /home/alfanick/Pictures/Lightroom/inbox \
  /home/alfanick/Pictures/mini-film-output \
  --profile 'Classic Film' \
  --profiles-root /path/to/profile-library \
  --nikon-wtu 192.168.1.50
```

The Nikon WTU receiver is native PTP/IP over TCP port `15740`; it does not use
external camera-control tools. During first-time setup it requests the camera's
pairing code, accepts the wizard, completes pairing, reconnects, and then waits
for transferred RAW objects. Non-RAW transfer objects are consumed silently so
they do not block the camera's transfer queue. mini-film persists a stable
initiator GUID in `$HOME/.cache/mini-film/nikon-wtu-guid` unless
`--nikon-wtu-guid` is provided, and records successful camera pairings in
`$HOME/.cache/mini-film/nikon-wtu-pairings.json`. Later daemon runs reuse the
cached camera/name/GUID identity and go straight to transfer mode. If a camera
was previously paired to a different identity, remove that pairing on the camera
or reuse the same computer name/GUID with `--nikon-wtu-name` and
`--nikon-wtu-guid`.

## Profile Sampler Contact Sheet

`sampler` renders one thumbnail per XMP file from `emulations/` and builds a structured contact sheet grouped by shared profile-name prefixes. For example, Kodak profiles are shown under progressively deeper headings like `Kodak`, `Kodak Portra`, `Kodak Portra 400`, and `Kodak Portra 400 Grainy`; indentation makes the level visible. Each thumbnail is developed with its profile-specific generated RawTherapee `.pp3` files, including Film Simulation for the Hald, which applies the LUT during RAW development, before the grain stage. Like `batch`, sampler renders half of the detected CPU threads in parallel by default; override with `--jobs N`. Thumbnail longest edge defaults to 512 px:

```sh
--jobs 8
--thumbnail-long-edge 768
--jpg-quality 92
--no-grain
```

Use a non-default convert binary or write a progressive sampler JPEG with:

```sh
--convert /path/to/convert
--progressive
```

## JPEG Export Options

`apply`, `batch`, and `daemon` support the same final JPG controls. `batch` and `daemon` also accept
`--output-format jpg|tiff`; TIFF batch output is written as 16-bit Zip-compressed `.tif`.
`sampler` supports `--jpg-quality`, `--jpeg-subsampling`, `--strip-metadata`, and
`--progressive` for generated sampler JPEGs, and accepts `.jpg/.jpeg` or `.html`
outputs (HTML mode produces a clickable gallery with lazy thumbnails).

```sh
--jpg-quality 90
--long-edge 3000
--jpeg-subsampling s444
--progressive-jpeg
--strip-metadata
```

Resize options:

- `--resize 3000x3000>` passes explicit GraphicsMagick resize geometry.
- `--long-edge 3000` constrains the longest edge to 3000 px.
- `--max-width 3000` constrains width only.
- `--max-height 2000` constrains height only.
- `--max-width 3000 --max-height 2000` constrains both dimensions.

Use one resize mode at a time: `--resize`, `--long-edge`, or `--max-width/--max-height`.

JPEG subsampling values:

- `s444`: best quality, no chroma subsampling
- `s422`: balanced horizontal chroma subsampling
- `s420`: smaller files with horizontal and vertical chroma subsampling

## Profile Selection

`--profile` is optional for `apply`, `batch`, `daemon`, and the desktop app. If
it is omitted entirely, mini-film asks RawTherapee to develop the RAW with its
default settings and does not apply Hald, PP3, profile adjustments, or
profile-derived grain. When provided, `--profile` accepts:

- a Hald PNG path
- a RawTherapee `.pp3` path
- an emulation XMP path containing `crs:Look` plus optional grain settings
- an emulation name, searched under `emulations/` using case-insensitive fuzzy matching
- a generated Hald name, searched under `--hald-dir`, which defaults to `$HOME/.cache/mini-film/hald`

RGBTable XMPs under `profiles/` are internal lookup tables. `apply`, `batch`, and `sampler` do not use them as user-facing emulations; they are only used to resolve linked `crs:Look` UUID/name references from emulation XMPs. `mini-film` generates or reuses a cached Hald from the linked profile under `$HOME/.cache/mini-film/hald`, generates temporary RawTherapee `.pp3` files for supported XMP adjustments and Film Simulation, lets RawTherapee apply the Hald, then applies the emulation grain settings.

## Profile Info

Print parsed details for a user-facing emulation or an internal RGBTable profile:

```sh
cargo run --release -- info \
  'Polaroid 600 v3 grainy' \
  --profiles-root /path/to/profile-library
```

`info` resolves emulation names under `emulations/`, direct emulation XMP paths, direct internal profile XMP paths, internal profile names under `profiles/`, and cached Hald PNGs under `--hald-dir`. For emulations, it prints the preset identity, linked Look, linked internal RGBTable profile, cached Hald path, profile-side tone/color/sharpening adjustments, and emulation-side grain/adjustments.

## RawTherapee PP3 Output

Print the generated RawTherapee PP3 for a profile:

```sh
cargo run --release -- pp3 \
  'Polaroid 600 v3 grainy' \
  --profiles-root /path/to/profile-library \
  --output polaroid-600-v3.pp3
```

`pp3` uses the same profile/emulation resolver as `info`. It writes to `/dev/stdout` by default, or to `--output`. The output contains the RawTherapee adjustment profile sections that mini-film would pass to `rawtherapee-cli`, followed by the Film Simulation section pointing at the cached Hald PNG.

## Nikon Picture Control Output

Fit an emulation XMP, internal RGBTable XMP, or Hald PNG into a Nikon classic `.NCP` Picture Control:

```sh
cargo run --release -- nikon \
  'Polaroid 600 v3 grainy' \
  --profiles-root /path/to/profile-library \
  --output polaroid-600-v3.ncp \
  --report polaroid-600-v3.ncp.txt
```

The `nikon` command writes a real classic NCP file using a neutral base Picture Control plus a fitted 257-point user-defined luminosity curve. It also estimates coarse saturation, hue, and sharpening fields from the profile. Use `--name 'Short Name'` to set the in-camera Picture Control name; NCP names are ASCII and short, so mini-film sanitizes and truncates them.

This is necessarily lossy. Nikon classic NCP does not store a full 3D LUT, RGBTable, Hald CLUT, or grain model. Color-specific film behavior is compressed into a 1D luma curve plus coarse sliders. Use the optional report to inspect mean/max luma and color error before trusting the result.

## Processing Split

RawTherapee handles:

- RAW development and Hald CLUT application through Film Simulation
- camera JPEG tone response, including visible Nikon Active D-Lighting effects,
  through RawTherapee histogram matching
- `Exposure2012` and `Contrast2012`; `Blacks2012`, `Shadows2012`,
  `Highlights2012`, and `Whites2012` map directly to RawTherapee Tone Equalizer
  bands 0, 1, 3, and 4, with its midtones band 2 left neutral
- `Saturation`, `Vibrance`
- `ToneCurvePV2012` and per-channel `ToneCurvePV2012Red/Green/Blue`
- `ParametricShadows/Darks/Lights/Highlights` and split points
- HSL `HueAdjustment*`, `SaturationAdjustment*`, `LuminanceAdjustment*`
- calibration-style `RedHue/RedSaturation`, `GreenHue/GreenSaturation`, `BlueHue/BlueSaturation`
- `Clarity2012` as a RawTherapee luminance-contrast approximation
- profile sharpening fields in generated `.pp3` files:
  `Sharpness`, `SharpenRadius`, `SharpenDetail`, `SharpenEdgeMasking`

Source-profile sharpening is applied before emulation sharpening. Explicit
emulation sharpening overrides the source, but an emulation that omits
sharpening preserves an explicit source setting. The minimal `5/0.6/10/0`
fallback is added only when neither layer defines sharpening.

mini-film internally handles:

- resolving emulation XMPs to internal RGBTable XMPs under `profiles/`
- decoding RGBTable payloads and generating RGBTable-only Hald PNGs
- procedural grain from Lightroom grain fields

ImageMagick/GraphicsMagick `convert` handles:

- final resize, bit depth, metadata stripping, JPEG quality/subsampling, progressive JPEG, TIFF Zip compression, and TIFF/JPEG encoding
- structured sampler contact sheet rendering from mini-film's generated SVG layout

## RAW Development

`mini-film` uses RawTherapee as its only RAW engine. TIFF outputs and explicit `--keep-intermediate` runs render a 16-bit TIFF intermediate with:

```sh
rawtherapee-cli -q -Y [-p generated.pp3 ...] -o intermediate.tif -t -b16 -c input.RAW
```

JPEG-bound `apply`, `batch`, and `sampler` runs ask RawTherapee for an 8-bit JPEG intermediate instead:

```sh
rawtherapee-cli -q -Y [-p generated.pp3 ...] -o intermediate.jpg -j95 -js3 -c input.RAW
```

Sampler also adds a temporary RawTherapee resize profile so each RAW development produces a thumbnail-sized JPEG instead of a full-size TIFF.

When a Nikon RAW carries `ActiveD-Lighting`, mini-film keeps that MakerNote value
in review metadata and shows it in the film simulation overlay. The embedded
camera JPEG already includes Nikon's proprietary rendering, so the auto-matched
curve derives its tonal effect from the camera output. Mini-film does not stack
an additional synthetic D-Lighting curve on top of that match.

Use a non-default RawTherapee binary path with:

```sh
--rawtherapee /path/to/rawtherapee-cli
```

## Grain

Lightroom grain fields are read from preset XMPs:

- `crs:GrainAmount`
- `crs:GrainSize`
- `crs:GrainFrequency`

Grain is rendered internally after RawTherapee. TIFF outputs use the 16-bit
grain path; JPEG outputs use the optimized 8-bit grain path. The default engine
is `legacy`, mini-film's older additive procedural renderer.

RFGR remains available as an opt-in engine. It is an original implementation
derived from the algorithm described in Newson, Faraj, Galerne, and Delon's IPOL
paper "Realistic Film Grain Rendering": an inhomogeneous Boolean model of random
disks, Gaussian filtering, and Monte Carlo sampling. It uses the existing
Lightroom-style `amount,size,frequency` controls, not new RFGR-specific knobs.
The implementation is derived from the paper's described algorithm and is not
copied from IPOL's GPL source archive.

RFGR engines detect visually monochrome developed images and switch to a shared
luminance grain path, so black-and-white exports do not get independent RGB
grain speckles.

For visual reference, see the
[grain engine comparison PDF](https://sam.nakarmamana.ch/mini-film-hidden/grain_engine_comparison.pdf).

Disable grain with:

```sh
--no-grain
```

Set deterministic variation with:

```sh
--grain-seed 42
```

When using a Hald PNG directly, pass grain manually:

```sh
--grain 30,45,45
```

or use a preset:

```sh
--grain-preset light
--grain-preset medium
--grain-preset heavy
```

Select the default legacy renderer explicitly with:

```sh
--grain-engine legacy
```

For a much faster cached approximation of the RFGR look, use:

```sh
--grain-engine rfgrfast
```

`rfgrfast` builds a seeded stochastic coverage cache for each RGB channel and
blurs it once instead of running the full per-pixel Monte Carlo disk sampler.
It still uses the same `amount,size,frequency` controls and the same
time-based or explicit grain seed flow, so batch/daemon/review outputs get
per-picture variation rather than reusing one fixed texture.

Select the exact RFGR engine explicitly with:

```sh
--grain-engine rfgr
```

## RAW Tone Matching

Every RAW render explicitly asks RawTherapee to generate an auto-matched tone
curve from the camera's embedded JPEG. Mini-film appends a partial PP3 layer
with `HistogramMatching=true` and `CurveFromHistogramMatching=false` after the
selected profile layers, so the behavior does not depend on the user's
RawTherapee preferences and a stale stored curve is not reused. This is the
same histogram-matching switch used by RawTherapee's bundled
`Auto-Matched Curve - ISO Low/Medium/High` profiles; mini-film handles ISO-based
denoising separately. JPEG and HEIC profile inputs do not receive RAW histogram
matching.

RawTherapee requires an adequately sized embedded preview for matching. If a
RAW has no usable preview, RawTherapee falls back to a linear curve.

## Color Noise Reduction

Since v3.1, `mini-film` can generate a short temporary RawTherapee pp3 block for
directional pyramid color denoising when source EXIF ISO is above a threshold.

- Controlled by `--color-noise-iso-threshold` on `apply`, `batch`, `daemon`, and
  `sampler`.
- Default threshold is `1600`.
- Set to `0` to disable color-noise processing.
- The ISO is read from raw EXIF; when no ISO is available, the step is skipped.

The thresholds use stepped levels:

- `ISO >= 25_600`: very strong color-denoise
- `ISO >= 6_400`: strong
- `ISO >= 1_600`: moderate (default threshold)
- below threshold: skipped

The denoise block is appended as an extra generated `[Directional Pyramid Denoising]`
section in the RawTherapee profile chain, after emulation/RAW adjustments and film
simulation and before mini-film procedural grain.

Example:

```sh
--color-noise-iso-threshold 1600   # default
--color-noise-iso-threshold 0      # disable
```

## RawTherapee Lens Corrections

Mini-film can optionally enable RawTherapee lens-correction controls. This is
off by default and available on `apply`, `batch`, `sampler`, and `daemon` via:

```sh
--lens-corrections
--lens-corrections all
--lens-corrections distortion,ca,vignetting
```

When no value is provided, `--lens-corrections` enables all supported items.
You can also pass any subset of:

- `distortion`
- `ca` or `chromatic-aberration`
- `vignetting` / `vignette`
- `all`

The generated section is inserted into the temporary pp3 stack as:

```ini
[LensProfile]
LcMode=lfauto
UseDistortion=<true|false>
UseVignette=<true|false>
UseCA=<true|false>
```

## Caveat

This does not fully clone Adobe Camera Raw. It decodes and applies the profile RGB table, maps supported profile tone/color/sharpening fields into generated RawTherapee `.pp3` files, uses RawTherapee for RAW development, and emulates Lightroom grain. Adobe-specific tone mapping, local contrast, sharpening, and camera matching may still differ.
