# record-prepress

`record-prepress` is the WASM-compatible plant-ready export boundary for
Bitneedle. Browser code may preview artwork, but final print PDFs must go
through this Rust core instead of ad hoc browser canvas output.

The core accepts a job manifest with a record-plant template, one artwork file
per print slot, and target print settings. It validates slot count, placed
resolution, supported source color, cutout geometry, and CMYK/ICC requirements
before export.

The optional native CLI is only a local development helper. It is not the
browser product path and it uses the same pure Rust exporter as WASM. Enable it
explicitly:

```bash
cargo run -p record-prepress --features native-cli -- validate --job plant-job.json
```

Example local helper usage:

```bash
cargo run -p record-prepress --features native-cli -- export --job plant-job.json --out plant-ready.pdf --icc-profile profile.icc
cargo run -p record-prepress --features native-cli -- preflight --pdf plant-ready.pdf --preflight-json native-preflight.json
```

The current exporter is intentionally strict: `target.iccProfile` and matching
ICC bytes are required for CMYK plant-ready export. Source artwork is normalized
to sRGB from embedded RGB ICC profiles when present, then the final raster is
converted to the target CMYK ICC profile with `moxcms` and embedded as the PDF
output intent. If a plant publishes a specific profile, pass those ICC bytes to
the exporter. If not, choose a house CMYK profile explicitly rather than relying
on browser RGB canvas output.

See `COLOR_PROFILE.md` for the focused CMYK/DPI validation command, golden files,
and profile fingerprint behavior.

The export result includes a machine-readable preflight report. It hard-fails
on invalid slot counts, low placed resolution, missing/invalid target CMYK ICC,
and unsupported source profile state. Semantic safety clearance is reported as
proof/validation data because flattened pixels cannot prove whether text or
logos are intentionally outside a safety area.

The native `preflight` helper performs local PDF structure checks and runs
Ghostscript when installed. If `veraPDF` is installed it is also used as the
external PDF/X conformance gate; otherwise the report records that PDF/X
conformance is not externally proven.
