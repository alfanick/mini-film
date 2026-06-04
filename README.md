# mini-film

`mini-film` applies Lightroom-style film profile XMPs from the command line.

It can:

- convert Adobe Camera Raw / Lightroom `crs:RGBTable` profile XMPs to 16-bit Hald CLUT PNGs
- develop a RAW file with `dcraw`
- apply a Hald CLUT with GraphicsMagick/ImageMagick `convert`
- read Lightroom preset XMPs that reference a profile and define grain
- add deterministic procedural film grain
- export either 10-bit TIFF or 8-bit JPEG

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

The ordinary RNI preset XMPs usually only reference a profile UUID and do not contain the table payload; `hald` skips those in directory mode.

## Apply A Complete Film Recipe

Use a Lightroom preset XMP that references a profile and defines grain:

```sh
cargo run --release -- apply input.RAW \
  --profile '../RNI FILMS 5 Negative - Pro/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root .. \
  -o output.tif
```

`output.tif` / `output.tiff` is exported as 10-bit TIFF.

```sh
cargo run --release -- apply input.RAW \
  --profile '../RNI FILMS 5 Negative - Pro/Kodak Portra 400 warm grainy.xmp' \
  --profiles-root .. \
  -o output.jpg
```

`output.jpg` / `output.jpeg` is exported as 8-bit JPEG.

## Profile Selection

`--profile` accepts:

- a Hald PNG path
- a profile XMP path containing `crs:RGBTable`
- a preset XMP path containing `crs:Look` plus optional grain settings
- a profile or preset name, searched under `--profiles-root`
- a generated Hald name, searched under `--hald-dir`

When a preset XMP references a profile, `mini-film` resolves the linked `crs:Look` UUID/name under `--profiles-root`, generates a temporary Hald, applies it, then applies the preset grain settings.

## RAW Color Handling

The default `dcraw` arguments are:

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

## Caveat

This does not fully clone Adobe Camera Raw. It decodes and applies the profile RGB table, uses `dcraw` for RAW development, and emulates Lightroom grain. Adobe-specific tone mapping and camera matching outside those data fields may still differ.
