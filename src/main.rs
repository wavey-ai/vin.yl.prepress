use anyhow::{bail, Context, Result};
use record_prepress::{
    artwork_info_from_bytes, build_prepress_plan, export_prepress_pdf, ArtworkInfo,
    PlantPrepressArtworkBytes, PlantPrepressJob,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        bail!("missing command");
    };
    let options = CliOptions::parse(&args[1..])?;
    match command {
        "validate" => {
            let job = required_job(&options, "validate")?;
            let artwork = collect_artwork_info(&job)?;
            let plan = build_prepress_plan(&job, &artwork)?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        "export" => {
            let out = options
                .out
                .as_ref()
                .context("export requires --out <plant-ready.pdf>")?;
            let job = required_job(&options, "export")?;
            let artwork = collect_artwork_bytes(&job)?;
            let icc_profile_bytes = read_icc_profile_bytes(&job, &options)?;
            let exported = export_prepress_pdf(&job, &artwork, icc_profile_bytes.as_deref())?;
            fs::write(out, exported.pdf_bytes)
                .with_context(|| format!("failed to write {}", out.display()))?;
            if let Some(preflight_json) = options.preflight_json.as_ref() {
                fs::write(
                    preflight_json,
                    format!("{}\n", serde_json::to_string_pretty(&exported.preflight)?),
                )
                .with_context(|| format!("failed to write {}", preflight_json.display()))?;
            }
            println!("{}", out.display());
        }
        "preflight" => {
            let pdf = options
                .pdf
                .as_ref()
                .context("preflight requires --pdf <plant-ready.pdf>")?;
            let report = native_preflight(pdf)?;
            let json = format!("{}\n", serde_json::to_string_pretty(&report)?);
            if let Some(preflight_json) = options.preflight_json.as_ref() {
                fs::write(preflight_json, &json)
                    .with_context(|| format!("failed to write {}", preflight_json.display()))?;
            } else {
                print!("{json}");
            }
            if let Some(failed) = report.checks.iter().find(|check| check.status == "fail") {
                bail!("native preflight failed: {}", failed.detail);
            }
        }
        _ => {
            print_usage();
            bail!("unknown command: {command}");
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct CliOptions {
    job: Option<PathBuf>,
    out: Option<PathBuf>,
    pdf: Option<PathBuf>,
    icc_profile: Option<PathBuf>,
    preflight_json: Option<PathBuf>,
}

impl CliOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut job = None;
        let mut out = None;
        let mut pdf = None;
        let mut icc_profile = None;
        let mut preflight_json = None;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--job" => {
                    index += 1;
                    job = Some(PathBuf::from(
                        args.get(index).context("--job requires a path")?,
                    ));
                }
                "--out" => {
                    index += 1;
                    out = Some(PathBuf::from(
                        args.get(index).context("--out requires a path")?,
                    ));
                }
                "--pdf" => {
                    index += 1;
                    pdf = Some(PathBuf::from(
                        args.get(index).context("--pdf requires a path")?,
                    ));
                }
                "--icc-profile" => {
                    index += 1;
                    icc_profile = Some(PathBuf::from(
                        args.get(index).context("--icc-profile requires a path")?,
                    ));
                }
                "--preflight-json" => {
                    index += 1;
                    preflight_json = Some(PathBuf::from(
                        args.get(index)
                            .context("--preflight-json requires a path")?,
                    ));
                }
                "-h" | "--help" => {
                    print_usage();
                    std::process::exit(0);
                }
                other => bail!("unknown option: {other}"),
            }
            index += 1;
        }
        Ok(Self {
            job,
            out,
            pdf,
            icc_profile,
            preflight_json,
        })
    }
}

fn print_usage() {
    eprintln!(
        "usage:\n  record-prepress validate --job plant-job.json\n  record-prepress export --job plant-job.json --out plant-ready.pdf [--icc-profile profile.icc] [--preflight-json report.json]\n  record-prepress preflight --pdf plant-ready.pdf [--preflight-json native-report.json]"
    );
}

fn required_job(options: &CliOptions, command: &str) -> Result<PlantPrepressJob> {
    let path = options
        .job
        .as_ref()
        .with_context(|| format!("{command} requires --job <plant-job.json>"))?;
    read_job(path)
}

fn read_job(path: &Path) -> Result<PlantPrepressJob> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid prepress job {}", path.display()))
}

fn collect_artwork_bytes(job: &PlantPrepressJob) -> Result<Vec<PlantPrepressArtworkBytes>> {
    job.slots
        .iter()
        .map(|slot| {
            let path = Path::new(&slot.artwork_path);
            let bytes = fs::read(path)
                .with_context(|| format!("failed to read artwork {}", path.display()))?;
            Ok(PlantPrepressArtworkBytes {
                slot_id: slot.id.clone(),
                bytes,
            })
        })
        .collect()
}

fn collect_artwork_info(job: &PlantPrepressJob) -> Result<Vec<ArtworkInfo>> {
    collect_artwork_bytes(job)?
        .iter()
        .map(|asset| {
            artwork_info_from_bytes(asset.slot_id.clone(), &asset.bytes)
                .with_context(|| format!("failed to inspect artwork slot {}", asset.slot_id))
        })
        .collect()
}

fn read_icc_profile_bytes(job: &PlantPrepressJob, options: &CliOptions) -> Result<Option<Vec<u8>>> {
    let path = options.icc_profile.clone().or_else(|| {
        job.target.icc_profile.as_ref().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(PathBuf::from(trimmed))
            }
        })
    });
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    fs::read(&path)
        .with_context(|| format!("failed to read ICC profile {}", path.display()))
        .map(Some)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativePreflightReport {
    pdf: String,
    checks: Vec<NativePreflightCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativePreflightCheck {
    id: String,
    status: String,
    summary: String,
    detail: String,
}

fn native_preflight(path: &Path) -> Result<NativePreflightReport> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let pdf_text = String::from_utf8_lossy(&bytes);
    let mut checks = Vec::new();

    checks.push(native_check(
        "pdf-header",
        if bytes.starts_with(b"%PDF-") {
            "pass"
        } else {
            "fail"
        },
        "PDF header is present",
        if bytes.starts_with(b"%PDF-") {
            "File starts with a PDF header.".to_string()
        } else {
            "File does not start with a PDF header.".to_string()
        },
    ));
    checks.push(native_check(
        "pdf-x-markers",
        if pdf_text.contains("/GTS_PDFXVersion")
            && pdf_text.contains("/OutputIntents")
            && pdf_text.contains("/S /GTS_PDFX")
            && pdf_text.contains("pdfxid:GTS_PDFXVersion")
        {
            "pass"
        } else {
            "fail"
        },
        "PDF/X markers and output intent are present",
        if pdf_text.contains("/GTS_PDFXVersion")
            && pdf_text.contains("/OutputIntents")
            && pdf_text.contains("/S /GTS_PDFX")
            && pdf_text.contains("pdfxid:GTS_PDFXVersion")
        {
            "Found Info/XMP PDF/X markers and an OutputIntent dictionary.".to_string()
        } else {
            "Missing one or more PDF/X marker, XMP marker, or OutputIntent dictionary.".to_string()
        },
    ));
    checks.push(native_check(
        "no-rgb-objects",
        if !pdf_text.contains("/DeviceRGB")
            && !pdf_text.contains("/CalRGB")
            && !pdf_text.contains("/Lab")
        {
            "pass"
        } else {
            "fail"
        },
        "No RGB PDF color objects are present",
        if !pdf_text.contains("/DeviceRGB")
            && !pdf_text.contains("/CalRGB")
            && !pdf_text.contains("/Lab")
        {
            "Object text contains no DeviceRGB, CalRGB, or Lab color spaces.".to_string()
        } else {
            "Object text contains an RGB/Lab color space marker.".to_string()
        },
    ));
    checks.push(native_check(
        "pdf-boxes",
        if pdf_text.contains("/MediaBox")
            && pdf_text.contains("/TrimBox")
            && pdf_text.contains("/BleedBox")
        {
            "pass"
        } else {
            "fail"
        },
        "MediaBox, TrimBox, and BleedBox are present",
        if pdf_text.contains("/MediaBox")
            && pdf_text.contains("/TrimBox")
            && pdf_text.contains("/BleedBox")
        {
            "Found page box dictionaries required by the plant-ready exporter.".to_string()
        } else {
            "Missing one or more MediaBox, TrimBox, or BleedBox marker.".to_string()
        },
    ));

    checks.push(run_ghostscript_preflight(path));
    checks.push(run_verapdf_preflight(path));

    Ok(NativePreflightReport {
        pdf: path.display().to_string(),
        checks,
    })
}

fn native_check(
    id: impl Into<String>,
    status: impl Into<String>,
    summary: impl Into<String>,
    detail: impl Into<String>,
) -> NativePreflightCheck {
    NativePreflightCheck {
        id: id.into(),
        status: status.into(),
        summary: summary.into(),
        detail: detail.into(),
    }
}

fn run_ghostscript_preflight(path: &Path) -> NativePreflightCheck {
    let output = Command::new("gs")
        .args([
            "-q",
            "-dSAFER",
            "-dBATCH",
            "-dNOPAUSE",
            "-dPDFSTOPONERROR",
            "-sDEVICE=nullpage",
        ])
        .arg(path)
        .output();
    match output {
        Ok(output) if output.status.success() => native_check(
            "ghostscript-parse",
            "pass",
            "Ghostscript parses the PDF without errors",
            clean_command_output(&output.stderr, &output.stdout)
                .unwrap_or_else(|| "gs completed successfully.".to_string()),
        ),
        Ok(output) => native_check(
            "ghostscript-parse",
            "fail",
            "Ghostscript rejected the PDF",
            clean_command_output(&output.stderr, &output.stdout)
                .unwrap_or_else(|| format!("gs exited with status {}", output.status)),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => native_check(
            "ghostscript-parse",
            "warn",
            "Ghostscript is not installed",
            "Install Ghostscript to run native structural PDF validation.".to_string(),
        ),
        Err(error) => native_check(
            "ghostscript-parse",
            "fail",
            "Ghostscript preflight could not run",
            error.to_string(),
        ),
    }
}

fn run_verapdf_preflight(path: &Path) -> NativePreflightCheck {
    let output = Command::new("verapdf")
        .args(["--format", "text"])
        .arg(path)
        .output();
    match output {
        Ok(output) if output.status.success() => native_check(
            "verapdf",
            "pass",
            "veraPDF validates PDF/X conformance",
            clean_command_output(&output.stdout, &output.stderr)
                .unwrap_or_else(|| "veraPDF completed successfully.".to_string()),
        ),
        Ok(output) => native_check(
            "verapdf",
            "fail",
            "veraPDF rejected PDF/X conformance",
            clean_command_output(&output.stdout, &output.stderr).unwrap_or_else(|| {
                format!("veraPDF exited with status {}", output.status)
            }),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => native_check(
            "verapdf",
            "warn",
            "veraPDF is not installed",
            "Install veraPDF to prove external PDF/X conformance. Internal PDF/X marker checks still ran."
                .to_string(),
        ),
        Err(error) => native_check(
            "verapdf",
            "fail",
            "veraPDF preflight could not run",
            error.to_string(),
        ),
    }
}

fn clean_command_output(primary: &[u8], secondary: &[u8]) -> Option<String> {
    let text = if primary.is_empty() {
        secondary
    } else {
        primary
    };
    let cleaned = String::from_utf8_lossy(text).trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}
