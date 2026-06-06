# mini-film

`mini-film` applies Lightroom-style film profile XMPs from the command line.

It can:

- convert Adobe Camera Raw / Lightroom `crs:RGBTable` profile XMPs to 16-bit Hald CLUT PNGs
- generate RawTherapee `.pp3` profiles for supported Camera Raw tone/color/sharpening adjustments
- develop a RAW file with `rawtherapee-cli`
- apply a Hald CLUT with GraphicsMagick/ImageMagick `convert`
- read Lightroom preset XMPs that reference a profile and define grain
- batch-process DNG/NEF folders into JPEGs
- render a profile sampler contact sheet for one RAW file
- inspect emulation/profile XMP adjustments
- print generated RawTherapee PP3 profiles
- fit XMP/Hald looks into experimental Nikon `.NCP` Picture Controls
- add deterministic procedural film grain
- export either 16-bit TIFF or 8-bit JPEG

## Example

Example interactive HTML sampler output:
<https://sam.nakarmamana.ch/mini-film/sampler.html>

Example JPEG sampler output:
<https://sam.nakarmamana.ch/mini-film/sampler.jpg>

This sampler was generated from my pictures using RNI All Films 5 Pro profiles:
<https://reallyniceimages.com/products/rni-all-films-5-pro-for-adobe-lightroom.html>

The RNI profiles are not part of this project. To use `mini-film` with those
emulations, obtain the profiles yourself and point `--profiles-root` at your
local profile library.

## Build

```sh
cargo build --release
```

## Convert XMP Profiles To Hald

Convert one profile XMP:

```sh
cargo run --release -- hald \
  '/home/alfanick/Pictures/RNI/profiles/Kodak Portra 400 normalised profile.xmp' \
  -o '../hald/Kodak Portra 400 normalised.hald.png' \
  --overwrite
```

Convert all profile XMPs under the parent directory:

```sh
cargo run --release -- hald /home/alfanick/Pictures/RNI/profiles --overwrite
```

The ordinary RNI preset XMPs usually only reference a profile UUID and do not contain the table payload; `hald` skips those in directory mode. When `-o/--output` is omitted, `hald` writes generated Hald PNGs under `$HOME/.cache/mini-film/hald`. Hald PNGs contain only the decoded RGBTable lookup. Profile XMPs that include extra Camera Raw settings print `adjustments=pp3` or `sharpening=pp3`; those settings are handled through generated RawTherapee profiles during `apply`, `batch`, and `sampler`.

## Apply A Complete Film Recipe

Use a Lightroom emulation XMP that references an internal profile and defines grain:

```sh
cargo run --release -- apply input.RAW \
  --profile '/home/alfanick/Pictures/RNI/emulations/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root /home/alfanick/Pictures/RNI \
  -o output.tif
```

If `--profile` is an emulation name, set `--profiles-root` to the RNI library directory that contains `emulations/` and `profiles/`:

```sh
cargo run -- apply \
  --output /home/alfanick/test.jpg \
  --profile 'Agfa Scala 200 + grainy' \
  --profiles-root /home/alfanick/Pictures/RNI \
  /home/alfanick/Pictures/Lightroom/2026/05/03/DSC_1812-10.dng
```

`output.tif` / `output.tiff` is exported as 16-bit Zip-compressed TIFF.

```sh
cargo run --release -- apply input.RAW \
  --profile '/home/alfanick/Pictures/RNI/emulations/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root /home/alfanick/Pictures/RNI \
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

## Batch Apply

Process every `.dng`, `.DNG`, `.nef`, and `.NEF` under an input directory and write JPGs or 16-bit TIFFs under an output directory:

```sh
cargo run --release -- batch \
  /home/alfanick/Pictures/Lightroom/2026/05/03 \
  /home/alfanick/batch-output \
  --profile 'Agfa Scala 200 + grainy' \
  --profiles-root /home/alfanick/Pictures/RNI \
  --output-format jpg \
  --jobs 8
```

The output directory is created if it does not exist. Nested input folders are preserved, and each RAW output uses the same relative path with a `.jpg` extension by default. Use `--output-format tiff` to write `.tif` files through the 16-bit Zip-compressed TIFF path.

By default, `batch` processes half of the detected CPU threads at once. On a 16-thread CPU that means 8 files in parallel. Override it with `--jobs N` when tuning for a different machine, output format, or memory budget.

`batch` shows two progress bars:

- total batch progress across files
- current file progress across RAW decode, Hald, grain, and final export steps

## Profile Sampler Contact Sheet

Render one RAW through every resolvable emulation XMP and write a labeled JPEG contact sheet:

```sh
cargo run --release -- sampler \
  /home/alfanick/Pictures/Lightroom/2026/05/03/DSC_1812-10.dng \
  --profiles-root /home/alfanick/Pictures/RNI \
  --output /home/alfanick/profile-sampler.jpg
```

`sampler` renders one thumbnail per XMP file from `emulations/` and builds a structured contact sheet grouped by shared profile-name prefixes. For example, Kodak profiles are shown under progressively deeper headings like `Kodak`, `Kodak Portra`, `Kodak Portra 400`, and `Kodak Portra 400 Grainy`; indentation makes the level visible. Each thumbnail is developed with its profile-specific generated RawTherapee `.pp3` files, including Film Simulation for the Hald, before the grain stage. Like `batch`, sampler renders half of the detected CPU threads in parallel by default; override with `--jobs N`. Thumbnail longest edge defaults to 512 px:

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

`apply` and `batch` support the same final JPG controls. `batch` also accepts
`--output-format jpg|tiff`; TIFF batch output is written as 16-bit Zip-compressed `.tif`.
`sampler` also
supports `--jpg-quality`, `--jpeg-subsampling`, `--strip-metadata`, and
`--progressive` for generated sampler JPEGs.

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

`--profile` accepts:

- a Hald PNG path
- a RawTherapee `.pp3` path
- an emulation XMP path containing `crs:Look` plus optional grain settings
- an emulation name, searched under `emulations/`
- a generated Hald name, searched under `--hald-dir`, which defaults to `$HOME/.cache/mini-film/hald`

RGBTable XMPs under `profiles/` are internal lookup tables. `apply`, `batch`, and `sampler` do not use them as user-facing emulations; they are only used to resolve linked `crs:Look` UUID/name references from emulation XMPs. `mini-film` generates or reuses a cached Hald from the linked profile under `$HOME/.cache/mini-film/hald`, generates temporary RawTherapee `.pp3` files for supported XMP adjustments and Film Simulation, lets RawTherapee apply the Hald, then applies the emulation grain settings.

## Profile Info

Print parsed details for a user-facing emulation or an internal RGBTable profile:

```sh
cargo run --release -- info \
  'Polaroid 600 v3 grainy' \
  --profiles-root /home/alfanick/Pictures/RNI
```

`info` resolves emulation names under `emulations/`, direct emulation XMP paths, direct internal profile XMP paths, internal profile names under `profiles/`, and cached Hald PNGs under `--hald-dir`. For emulations, it prints the preset identity, linked Look, linked internal RGBTable profile, cached Hald path, profile-side tone/color/sharpening adjustments, and emulation-side grain/adjustments.

## RawTherapee PP3 Output

Print the generated RawTherapee PP3 for a profile:

```sh
cargo run --release -- pp3 \
  'Polaroid 600 v3 grainy' \
  --profiles-root /home/alfanick/Pictures/RNI \
  --output polaroid-600-v3.pp3
```

`pp3` uses the same profile/emulation resolver as `info`. It writes to `/dev/stdout` by default, or to `--output`. The output contains the RawTherapee adjustment profile sections that mini-film would pass to `rawtherapee-cli`, followed by the Film Simulation section pointing at the cached Hald PNG.

## Nikon Picture Control Output

Fit an emulation XMP, internal RGBTable XMP, or Hald PNG into a Nikon classic `.NCP` Picture Control:

```sh
cargo run --release -- nikon \
  'Polaroid 600 v3 grainy' \
  --profiles-root /home/alfanick/Pictures/RNI \
  --output polaroid-600-v3.ncp \
  --report polaroid-600-v3.ncp.txt
```

The `nikon` command writes a real classic NCP file using a neutral base Picture Control plus a fitted 257-point user-defined luminosity curve. It also estimates coarse saturation, hue, and sharpening fields from the profile. Use `--name 'Short Name'` to set the in-camera Picture Control name; NCP names are ASCII and short, so mini-film sanitizes and truncates them.

This is necessarily lossy. Nikon classic NCP does not store a full 3D LUT, RGBTable, Hald CLUT, or grain model. Color-specific film behavior is compressed into a 1D luma curve plus coarse sliders. Use the optional report to inspect mean/max luma and color error before trusting the result.

## Processing Split

RawTherapee handles:

- RAW development and Hald CLUT application through Film Simulation
- `Exposure2012`, `Contrast2012`, `Highlights2012`, `Shadows2012`, `Whites2012`, `Blacks2012`
- `Saturation`, `Vibrance`
- `ToneCurvePV2012` and per-channel `ToneCurvePV2012Red/Green/Blue`
- `ParametricShadows/Darks/Lights/Highlights` and split points
- HSL `HueAdjustment*`, `SaturationAdjustment*`, `LuminanceAdjustment*`
- calibration-style `RedHue/RedSaturation`, `GreenHue/GreenSaturation`, `BlueHue/BlueSaturation`
- `Clarity2012` as a RawTherapee luminance-contrast approximation
- profile sharpening fields in generated `.pp3` files:
  `Sharpness`, `SharpenRadius`, `SharpenDetail`, `SharpenEdgeMasking`

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

Use a non-default RawTherapee binary path with:

```sh
--rawtherapee /path/to/rawtherapee-cli
```

## Grain

Lightroom grain fields are read from preset XMPs:

- `crs:GrainAmount`
- `crs:GrainSize`
- `crs:GrainFrequency`

Grain is rendered internally after RawTherapee. TIFF outputs use the 16-bit grain path; JPEG outputs use the optimized 8-bit grain path. Disable it with:

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

## Caveat

This does not fully clone Adobe Camera Raw. It decodes and applies the profile RGB table, maps supported profile tone/color/sharpening fields into generated RawTherapee `.pp3` files, uses RawTherapee for RAW development, and emulates Lightroom grain. Adobe-specific tone mapping, local contrast, sharpening, and camera matching may still differ.
