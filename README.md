# mini-film

`mini-film` applies Lightroom-style film profile XMPs from the command line.

It can:

- convert Adobe Camera Raw / Lightroom `crs:RGBTable` profile XMPs to 16-bit Hald CLUT PNGs
- bake profile-side Camera Raw tone/color adjustments into generated Hald CLUTs
- develop a RAW file with `dcraw`
- apply a Hald CLUT with GraphicsMagick/ImageMagick `convert`
- read Lightroom preset XMPs that reference a profile and define grain
- batch-process DNG/NEF folders into JPEGs
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

Use a Lightroom preset XMP that references a profile and defines grain:

```sh
cargo run --release -- apply input.RAW \
  --profile '../RNI FILMS 5 Negative - Pro/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root .. \
  -o output.tif
```

If the preset path sits next to its matching `- profiles` directory, no `--profiles-root` is needed:

```sh
cargo run -- apply \
  --output /home/alfanick/test.jpg \
  --profile '../RNI FILMS 5 BW - Pro/Agfa Scala 200 faded plus grainy.xmp' \
  /home/alfanick/Pictures/Lightroom/2026/05/03/DSC_1812-10.dng
```

`output.tif` / `output.tiff` is exported as 16-bit TIFF.

```sh
cargo run --release -- apply input.RAW \
  --profile '../RNI FILMS 5 Negative - Pro/Kodak Portra 400 warm grainy.xmp' \
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
  --profile '../RNI FILMS 5 BW - Pro/Agfa Scala 200 faded plus grainy.xmp'
```

The output directory is created if it does not exist. Nested input folders are preserved, and each RAW output uses the same relative path with a `.jpg` extension.

`batch` shows two progress bars:

- total batch progress across files
- current file progress across RAW decode, Hald/sharpening, grain, and JPEG export steps

## Profile Selection

`--profile` accepts:

- a Hald PNG path
- a profile XMP path containing `crs:RGBTable`
- a preset XMP path containing `crs:Look` plus optional grain settings
- a profile or preset name, searched under `--profiles-root`
- a generated Hald name, searched under `--hald-dir`

When a preset XMP references a profile, `mini-film` resolves the linked `crs:Look` UUID/name under `--profiles-root`, generates a temporary Hald, applies it, then applies the preset grain settings.

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

When using a Hald PNG or non-grain profile directly, pass grain manually:

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
