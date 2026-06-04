# rgbtable2hald

Convert Adobe Camera Raw / Lightroom `crs:RGBTable` XMP profiles into 16-bit Hald CLUT PNGs.

This is meant for "profile" XMPs that contain both:

- `crs:RGBTable="..."`
- `crs:Table_<id>="..."`

The ordinary RNI preset XMPs usually only reference a profile UUID and do not contain the table payload; those are skipped in directory mode.

## Build

```sh
cargo build --release
```

## Convert One Profile

```sh
cargo run --release -- \
  '../RNI FILMS 5 Negative - Pro - profiles/Kodak Portra 400 normalised profile.xmp' \
  -o '../hald/Kodak Portra 400 normalised.hald.png' \
  --overwrite
```

## Convert All Profiles

```sh
cargo run --release -- .. -o ../hald --overwrite
```

The default `--hald-level 8` writes a 512x512 PNG representing a 64x64x64 CLUT. The embedded RNI tables I tested are 32x32x32, so the converter resamples them with trilinear interpolation.

## Apply With GraphicsMagick

```sh
dcraw -T -6 -W -o 1 input.raw
convert input.tiff -hald-clut '../hald/Kodak Portra 400 normalised profile.hald.png' output.tiff
```

## Caveat

The converter decodes the Adobe RGB table structure and writes the table values as a Hald CLUT. It does not emulate the full Adobe Camera Raw pipeline around the profile. These RNI tables report `primaries=0` and `gamma=1`, which correspond to sRGB primaries and sRGB gamma in the DNG SDK enum, so they are plausible for direct CLUT use. Profiles using other primaries/gamma values may need additional color-space handling before or after the Hald transform.
