# mini-film

`mini-film` applies Lightroom-style film profile XMPs from the command line.

It can:

- convert Adobe Camera Raw / Lightroom `crs:RGBTable` profile XMPs to 16-bit Hald CLUT PNGs
- bake profile-side Camera Raw tone/color adjustments into generated Hald CLUTs
- develop a RAW file with `dcraw`
- apply a Hald CLUT with GraphicsMagick/ImageMagick `convert`
- read Lightroom preset XMPs that reference a profile and define grain
- batch-process DNG/NEF folders into JPEGs
- render a profile sampler contact sheet for one RAW file
- add deterministic procedural film grain
- export either 16-bit TIFF or 8-bit JPEG

## Build

```sh
cargo build --release
```

## Convert XMP Profiles To Hald

Convert one profile XMP:

```sh
cargo run --release -- hald \
  '../RNI FILMS 5 Negative - Pro - profiles/Kodak Portra 400 normalised profile.xmp' \
  -o '../hald/Kodak Portra 400 normalised.hald.png' \
  --overwrite
```

Convert all profile XMPs under the parent directory:

```sh
cargo run --release -- hald .. -o ../hald --overwrite
```

The ordinary RNI preset XMPs usually only reference a profile UUID and do not contain the table payload; `hald` skips those in directory mode. Profile XMPs that include extra Camera Raw settings print `adjustments=baked`; those settings are folded into the generated Hald along with the RGB table.

## Apply A Complete Film Recipe

Use a Lightroom emulation XMP that references an internal profile and defines grain:

```sh
cargo run --release -- apply input.RAW \
  --profile '../emulations/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root .. \
  -o output.tif
```

If the emulation and profile directories are siblings of the project directory, no `--profiles-root` is needed:

```sh
cargo run -- apply \
  --output /home/alfanick/test.jpg \
  --profile '../emulations/Agfa Scala 200 + grainy.xmp' \
  /home/alfanick/Pictures/Lightroom/2026/05/03/DSC_1812-10.dng
```

`output.tif` / `output.tiff` is exported as 16-bit TIFF.

```sh
cargo run --release -- apply input.RAW \
  --profile '../emulations/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root .. \
  -o output.jpg
```

`output.jpg` / `output.jpeg` is exported as 8-bit JPEG.

## Batch Apply

Process every `.dng`, `.DNG`, `.nef`, and `.NEF` under an input directory and write JPEGs under an output directory:

```sh
cargo run --release -- batch \
  /home/alfanick/Pictures/Lightroom/2026/05/03 \
  /home/alfanick/batch-output \
  --profile '../emulations/Agfa Scala 200 + grainy.xmp'
```

The output directory is created if it does not exist. Nested input folders are preserved, and each RAW output uses the same relative path with a `.jpg` extension.

`batch` shows two progress bars:

- total batch progress across files
- current file progress across RAW decode, Hald/sharpening, grain, and JPEG export steps

## Profile Sampler Contact Sheet

Render one RAW through every resolvable emulation XMP and write a labeled JPEG contact sheet:

```sh
cargo run --release -- sampler \
  /home/alfanick/Pictures/Lightroom/2026/05/03/DSC_1812-10.dng \
  --profiles-root .. \
  --output /home/alfanick/profile-sampler.jpg
```

`sampler` develops the RAW once, renders one thumbnail per XMP file from `emulations/`, and uses `montage` to build a contact sheet with six thumbnails per row. Each label is relative to the emulation directory. Thumbnail longest edge defaults to 512 px:

```sh
--thumbnail-long-edge 768
--jpg-quality 92
--no-grain
```

Use a non-default montage binary with:

```sh
--montage /path/to/montage
--progressive
```

## JPEG Export Options

`apply` and `batch` support the same final JPEG controls. `sampler` also
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
- an emulation XMP path containing `crs:Look` plus optional grain settings
- an emulation name, searched under `emulations/`
- a generated Hald name, searched under `--hald-dir`

RGBTable XMPs under `profiles/` are internal lookup tables. `apply`, `batch`, and `sampler` do not use them as user-facing emulations; they are only used to resolve linked `crs:Look` UUID/name references from emulation XMPs. `mini-film` generates a temporary Hald from the linked profile, applies it, then applies the emulation grain settings.

## Profile Adjustments

Linked profile XMPs can contain more than `crs:RGBTable`. `mini-film` bakes supported profile-side adjustments into the Hald so `convert -hald-clut` applies the whole color recipe in one pass.

Supported as CLUT adjustments:

- `Exposure2012`, `Contrast2012`, `Highlights2012`, `Shadows2012`, `Whites2012`, `Blacks2012`
- `Saturation`, `Vibrance`
- `ToneCurvePV2012` and per-channel `ToneCurvePV2012Red/Green/Blue`
- `ParametricShadows/Darks/Lights/Highlights` and split points
- HSL `HueAdjustment*`, `SaturationAdjustment*`, `LuminanceAdjustment*`
- calibration-style `RedHue/RedSaturation`, `GreenHue/GreenSaturation`, `BlueHue/BlueSaturation`
- `Clarity2012` as a global midtone contrast approximation
- profile sharpening fields as a spatial `convert -unsharp` pass after Hald application:
  `Sharpness`, `SharpenRadius`, `SharpenDetail`, `SharpenEdgeMasking`

Texture is not faithfully representable in a Hald CLUT and is not applied. Sharpening is applied outside the Hald because it is spatial; the mapping to `convert -unsharp` is approximate.

## RAW Engine And Color Handling

By default, `mini-film` uses:

```sh
--raw-engine auto
```

`auto` tries `rawtherapee-cli` first and falls back to `dcraw` if RawTherapee is unavailable or fails for a file. RawTherapee renders a 16-bit TIFF intermediate with its neutral command-line defaults:

```sh
rawtherapee-cli -q -Y -o intermediate.tif -t -b16 -c input.RAW
```

Force a specific engine with:

```sh
--raw-engine rawtherapee
--raw-engine dcraw
```

Use a non-default RawTherapee binary path with:

```sh
--rawtherapee /path/to/rawtherapee-cli
```

When `--raw-engine dcraw` is used, the default `dcraw` arguments are:

```sh
-T -6 -W -w -o 1
```

That means:

- `-6`: 16-bit working TIFF
- `-W`: no automatic brightening
- `-w`: camera/as-shot white balance
- `-o 1`: sRGB output space

This matches the decoded RNI tables in this directory, which report sRGB primaries and sRGB gamma in the DNG SDK RGB-table metadata. For cameras/workflows that need a specific input profile, pass it through to `dcraw`:

```sh
cargo run --release -- apply input.RAW \
  --profile 'Kodak Portra 400 warm grainy' \
  --profiles-root .. \
  --camera-profile embed \
  -o output.tif
```

or:

```sh
--camera-profile /path/to/camera.icc
```

`--camera-profile` and `--dcraw-args` apply only to the dcraw engine.

## Grain

Lightroom grain fields are read from preset XMPs:

- `crs:GrainAmount`
- `crs:GrainSize`
- `crs:GrainFrequency`

Grain is rendered on a 16-bit intermediate using Gaussian noise plus smooth procedural modulation from the Rust `noise` crate. Disable it with:

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

This does not fully clone Adobe Camera Raw. It decodes and applies the profile RGB table, bakes supported profile tone/color fields into the Hald, uses RawTherapee or dcraw for RAW development, and emulates Lightroom grain. Adobe-specific tone mapping, local contrast, sharpening, and camera matching may still differ.
