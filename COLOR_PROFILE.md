# Color Profile Validation

Last validated: 2026-06-05

`record-prepress` keeps plant-ready color handling inside the Rust export path.
Source artwork is decoded to sRGB for compositing, rasterized at the target DPI,
then converted to the target CMYK ICC profile with `moxcms` before the PDF image
stream and output intent are written.

## Focused CMYK/DPI Test

Run the isolated color and DPI golden test:

```bash
cargo test -p record-prepress color_dpi_raster_and_cmyk_pipeline_matches_goldens -- --nocapture
```

This is intentionally separate from the Jungle template proof test. It uses a
synthetic one-inch fixture so the assertions target the color and raster code:

- `25.4 mm` at `300 DPI` must become exactly `300 x 300 px`.
- The placed artwork effective resolution must be exactly `300 ppi`.
- The no-scale RGB raster must match the source RGB pixels byte-for-byte.
- `rgb_to_target_cmyk` must convert the raster through the active CMYK ICC
  profile.
- Raw CMYK bytes must match the binary golden when the profile fingerprint
  matches.
- The CMYK channel preview PNG must match the visual golden.

The validation passed on 2026-06-05 with:

```text
test tests::color_dpi_raster_and_cmyk_pipeline_matches_goldens ... ok
```

The current CMYK golden is tied to this profile fingerprint:

```text
fingerprint=fnv1a64-55f1dfb21bd4d804-len-55280
```

## Golden Files

The focused test owns these files under `goldenfiles/prepress/`:

- `color-dpi-cmyk-source.png`: original RGB color grid.
- `color-dpi-cmyk-raster-rgb.png`: post-DPI raster before CMYK conversion.
- `color-dpi-cmyk-output.cmyk`: raw converted CMYK bytes.
- `color-dpi-cmyk-channel-preview.png`: visual preview of C, M, Y, and K
  channels from the converted bytes.
- `color-dpi-cmyk-profile.txt`: CMYK profile fingerprint used for the raw CMYK
  golden.

For a valid no-scale DPI run, `color-dpi-cmyk-source.png` and
`color-dpi-cmyk-raster-rgb.png` should be visually identical. The channel preview
is a four-quadrant image: cyan, magenta, yellow, then black.

## Profile Selection

The test uses `BITNEEDLE_TEST_CMYK_ICC` when set. If that is absent, it tries the
macOS Generic CMYK profile:

```bash
BITNEEDLE_TEST_CMYK_ICC="/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc" \
cargo test -p record-prepress color_dpi_raster_and_cmyk_pipeline_matches_goldens -- --nocapture
```

If the output includes `skipping CMYK golden section`, the test still validated
DPI and RGB rasterization, but it did not compare the raw CMYK golden. Use the
same ICC profile as `color-dpi-cmyk-profile.txt`, or intentionally re-bless the
golden for the new profile.

## Re-Blessing

Only re-bless goldens when the intended profile or conversion behavior changes:

```bash
BITNEEDLE_UPDATE_PREPRESS_GOLDENS=1 \
cargo test -p record-prepress color_dpi_raster_and_cmyk_pipeline_matches_goldens -- --nocapture
```

After re-blessing, inspect the changed files in `goldenfiles/prepress/`. A
conversion-code change should alter `color-dpi-cmyk-output.cmyk` and usually
`color-dpi-cmyk-channel-preview.png`; a DPI/raster bug may alter
`color-dpi-cmyk-raster-rgb.png`.

## Supplier Alignment Proofs

The proof-pack tests verify that the Jungle-style alignment language is reusable
across cached supplier dimensions. Run:

```bash
cargo test -p record-prepress cached_supplier_alignment_proofs_match_goldens -- --nocapture
```

That test writes SVG goldens with the same artwork placed under visible proof
guides for:

- The Jungle Record Press 12 in A/B label sheet, `126 x 232 mm`.
- United Record Pressing 7 in large-hole two-up sheet, `228.6 x 149.2 mm`.
- Memphis Record Pressing 7 in single-label sheet, `127 x 127 mm`.
- Celebrate Records 12 in picture label, `298 x 298 mm`.

Each SVG should contain an artwork layer, supplier guide rings, alignment marks,
slot labels, and a CMYK process swatch. The geometry comes from the cached
supplier template, not from a fixed Jungle page.

## Jungle Pack Golden

Run the pack test:

```bash
cargo test -p record-prepress jungle_manufacturing_pack_artifacts_match_goldens -- --nocapture
```

It generates and checks the non-PDF pack goldens:

- `jungle-pack-proof.svg`: artwork laid onto the Jungle A/B sheet with visible
  trim, bleed, safety, alignment, A/B labels, and FOGRA39 CMYK swatch.
- `jungle-pack-record-plant-spec.json`: machine-readable pack/spec artifact.
- `jungle-pack-preflight.md`: human-readable preflight.
- `jungle-pack-readme.md`: plant-facing instruction note.

The test also builds `plant-ready.pdf` in memory and validates that it has the
ICC output intent/PDF/X markers while omitting alignment marks, swatches, slot
labels, and source guide identifiers. The generated PDF is not committed as a
golden file.
