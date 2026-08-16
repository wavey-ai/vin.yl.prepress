use flate2::write::ZlibEncoder;
use flate2::Compression;
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, ImageDecoder, ImageFormat, RgbaImage};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
use record_plant::{
    ExternalRecordPlantTemplateInput, GuideGeometry, GuideLayerKind, RecordPlantTemplateKind,
    RectMm, MM_PER_INCH,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Cursor;
use std::io::Write;

const DEFAULT_TARGET_DPI: u16 = 300;
const POINTS_PER_INCH: f64 = 72.0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressJob {
    pub template: ExternalRecordPlantTemplateInput,
    #[serde(default)]
    pub slots: Vec<PlantPrepressArtworkSlot>,
    #[serde(default)]
    pub target: PlantPrepressTarget,
    #[serde(default)]
    pub safety_clearance: PlantPrepressSafetyClearance,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressArtworkSlot {
    pub id: String,
    pub artwork_path: String,
    #[serde(default)]
    pub placement_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressTarget {
    #[serde(default)]
    pub dpi: Option<u16>,
    #[serde(default)]
    pub color_mode: Option<String>,
    #[serde(default)]
    pub icc_profile: Option<String>,
    #[serde(default)]
    pub source_rgb_profile: Option<String>,
    #[serde(default)]
    pub pdf_standard: Option<String>,
    #[serde(default)]
    pub output_condition_identifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressSafetyClearance {
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkInfo {
    pub slot_id: String,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlantPrepressArtworkBytes {
    pub slot_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlantPrepressPdf {
    pub plan: PlantPrepressPlan,
    pub preflight: PlantPrepressPreflightReport,
    pub pdf_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressPlan {
    pub template_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_version: Option<String>,
    pub template_name: String,
    pub page_width_mm: f64,
    pub page_height_mm: f64,
    pub page_width_px: u32,
    pub page_height_px: u32,
    pub target_dpi: u16,
    pub color_mode: String,
    pub source_rgb_profile: String,
    pub pdf_standard: String,
    pub icc_profile: String,
    pub output_condition_identifier: String,
    pub safety_clearance_confirmed: bool,
    pub slots: Vec<PlantPrepressSlotPlan>,
    pub bleed_areas: Vec<PlantPrepressPlacement>,
    pub trim_areas: Vec<PlantPrepressPlacement>,
    pub cutouts: Vec<PlantPrepressPlacement>,
    pub safety_areas: Vec<PlantPrepressPlacement>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressSlotPlan {
    pub slot_id: String,
    pub artwork_path: String,
    pub placement_index: usize,
    pub placement: PlantPrepressPlacement,
    pub effective_ppi: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressPlacement {
    pub x_mm: f64,
    pub y_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    pub shape: PlantPrepressPlacementShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlantPrepressPlacementShape {
    Rect,
    Circle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressPreflightReport {
    pub checks: Vec<PlantPrepressPreflightCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantPrepressPreflightCheck {
    pub id: String,
    pub status: PlantPrepressPreflightStatus,
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlantPrepressPreflightStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlantPrepressValidationError {
    issues: Vec<String>,
}

impl PlantPrepressValidationError {
    pub fn new(issues: Vec<String>) -> Self {
        Self { issues }
    }

    pub fn issues(&self) -> &[String] {
        &self.issues
    }
}

impl fmt::Display for PlantPrepressValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plant prepress validation failed")?;
        for issue in &self.issues {
            write!(f, "; {issue}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlantPrepressValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlantPrepressExportError {
    message: String,
}

impl PlantPrepressExportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PlantPrepressExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PlantPrepressExportError {}

impl From<PlantPrepressValidationError> for PlantPrepressExportError {
    fn from(error: PlantPrepressValidationError) -> Self {
        Self::new(error.to_string())
    }
}

pub fn build_prepress_plan(
    job: &PlantPrepressJob,
    artwork_infos: &[ArtworkInfo],
) -> Result<PlantPrepressPlan, PlantPrepressValidationError> {
    let mut issues = Vec::new();
    let bleed_areas = guide_layer_placements(&job.template, GuideLayerKind::Bleed);
    let trim_areas = guide_layer_placements(&job.template, GuideLayerKind::Trim);
    let placements = if !bleed_areas.is_empty() {
        bleed_areas.clone()
    } else if !trim_areas.is_empty() {
        trim_areas.clone()
    } else {
        vec![rect_placement(job.template.document)]
    };
    let cutouts = template_cutouts(&job.template);
    let safety_areas = template_safety_areas(&job.template);
    let target_dpi = target_dpi(job);
    let color_mode = normalized_color_mode(job);
    let source_rgb_profile = normalized_source_rgb_profile(job);
    let pdf_standard = normalized_pdf_standard(job);
    let icc_profile = job
        .target
        .icc_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let template_output_condition = job
        .template
        .requirements
        .output_condition_identifier
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if job.template.id.trim().is_empty() {
        issues.push("template id is required".to_string());
    }
    if !(job.template.document.width > 0.0 && job.template.document.height > 0.0) {
        issues.push("template document width/height must be positive".to_string());
    }
    if placements.is_empty() {
        issues.push("template has no supported bleed/trim placement geometry".to_string());
    }
    if job.slots.len() != placements.len() {
        issues.push(format!(
            "template has {} print slot{} but job provides {} artwork slot{}",
            placements.len(),
            if placements.len() == 1 { "" } else { "s" },
            job.slots.len(),
            if job.slots.len() == 1 { "" } else { "s" },
        ));
    }
    if color_mode != "cmyk" && color_mode != "grayscale" {
        issues.push(format!(
            "plant-ready export target colorMode must be CMYK or grayscale, got {color_mode}",
        ));
    }
    if icc_profile.is_empty() {
        issues.push("plant-ready PDF export requires target.iccProfile".to_string());
    }
    if !template_accepts_pdf(&job.template) {
        issues.push("plant-ready export requires a template that accepts PDF output".to_string());
    }
    if !template_accepts_color_mode(&job.template, &color_mode) {
        issues.push(format!(
            "template requirements do not list {color_mode} as an accepted color mode",
        ));
    }
    if !job.template.requirements.keep_template_layer_out_of_final {
        issues.push(
            "template requirements must state template/guide layers are kept out of final export"
                .to_string(),
        );
    }
    if source_rgb_profile != "srgb" {
        issues.push(format!(
            "plant-ready export currently supports sRGB source compositing only, got target.sourceRgbProfile {source_rgb_profile}",
        ));
    }
    if !pdf_standard.starts_with("pdf/x") {
        issues.push(format!(
            "plant-ready export requires a PDF/X target standard, got {pdf_standard}",
        ));
    }
    if !safety_areas.is_empty() && !job.safety_clearance.confirmed {
        issues.push(format!(
            "plant-ready export requires safetyClearance.confirmed for {} template safety area{}",
            safety_areas.len(),
            if safety_areas.len() == 1 { "" } else { "s" },
        ));
    }

    let info_by_slot: HashMap<&str, &ArtworkInfo> = artwork_infos
        .iter()
        .map(|info| (info.slot_id.as_str(), info))
        .collect();
    let mut used_placements = HashSet::new();
    let mut slot_plans = Vec::new();

    for (slot_index, slot) in job.slots.iter().enumerate() {
        let slot_id = slot.id.trim();
        if slot_id.is_empty() {
            issues.push(format!("artwork slot {slot_index} has no id"));
            continue;
        }
        if slot.artwork_path.trim().is_empty() {
            issues.push(format!("artwork slot {slot_id} has no artworkPath"));
        }
        let placement_index = slot.placement_index.unwrap_or(slot_index);
        if placement_index >= placements.len() {
            issues.push(format!(
                "artwork slot {slot_id} references placementIndex {placement_index}, but template only has {} placement{}",
                placements.len(),
                if placements.len() == 1 { "" } else { "s" },
            ));
            continue;
        }
        if !used_placements.insert(placement_index) {
            issues.push(format!(
                "multiple artwork slots target placementIndex {placement_index}",
            ));
            continue;
        }
        let placement = placements[placement_index];
        let Some(info) = info_by_slot.get(slot_id) else {
            issues.push(format!("artwork dimensions missing for slot {slot_id}"));
            continue;
        };
        let effective_ppi = effective_ppi(info, placement);
        if effective_ppi + f64::EPSILON < f64::from(target_dpi) {
            issues.push(format!(
                "artwork slot {slot_id} is {:.1} ppi at placed size, below target {target_dpi} ppi",
                effective_ppi,
            ));
        }
        slot_plans.push(PlantPrepressSlotPlan {
            slot_id: slot_id.to_string(),
            artwork_path: slot.artwork_path.clone(),
            placement_index,
            placement,
            effective_ppi,
        });
    }

    if !issues.is_empty() {
        return Err(PlantPrepressValidationError::new(issues));
    }

    Ok(PlantPrepressPlan {
        template_id: job.template.id.clone(),
        template_version: job.template.version.clone(),
        template_name: job.template.name.clone(),
        page_width_mm: job.template.document.width,
        page_height_mm: job.template.document.height,
        page_width_px: mm_to_px(job.template.document.width, target_dpi),
        page_height_px: mm_to_px(job.template.document.height, target_dpi),
        target_dpi,
        color_mode,
        source_rgb_profile,
        pdf_standard,
        icc_profile: icc_profile.to_string(),
        output_condition_identifier: job
            .target
            .output_condition_identifier
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or(template_output_condition)
            .unwrap_or(icc_profile)
            .to_string(),
        safety_clearance_confirmed: job.safety_clearance.confirmed,
        slots: slot_plans,
        bleed_areas,
        trim_areas,
        cutouts,
        safety_areas,
    })
}

pub fn artwork_info_from_bytes(
    slot_id: impl Into<String>,
    bytes: &[u8],
) -> Result<ArtworkInfo, PlantPrepressExportError> {
    let image = image::load_from_memory(bytes).map_err(|error| {
        PlantPrepressExportError::new(format!("failed to decode artwork: {error}"))
    })?;
    Ok(ArtworkInfo {
        slot_id: slot_id.into(),
        width_px: image.width(),
        height_px: image.height(),
    })
}

pub fn export_prepress_pdf(
    job: &PlantPrepressJob,
    artworks: &[PlantPrepressArtworkBytes],
    icc_profile_bytes: Option<&[u8]>,
) -> Result<PlantPrepressPdf, PlantPrepressExportError> {
    let decoded_artworks = artworks
        .iter()
        .map(DecodedArtwork::from_asset)
        .collect::<Result<Vec<_>, _>>()?;
    let artwork_infos = decoded_artworks
        .iter()
        .map(|artwork| ArtworkInfo {
            slot_id: artwork.slot_id.clone(),
            width_px: artwork.width,
            height_px: artwork.height,
        })
        .collect::<Vec<_>>();
    let plan = build_prepress_plan(job, &artwork_infos)?;
    let icc_profile_bytes = icc_profile_bytes.filter(|bytes| !bytes.is_empty());
    if icc_profile_bytes.is_none() {
        return Err(PlantPrepressExportError::new(
            "plant-ready PDF export requires ICC profile bytes",
        ));
    }
    let page_rgb = rasterize_prepress_page(&plan, &decoded_artworks)?;
    let (image_bytes, channels, fallback_color_space, conversion_method) =
        if plan.color_mode == "grayscale" {
            (
                rgb_to_grayscale(&page_rgb),
                1,
                "/DeviceGray",
                "sRGB raster converted to grayscale luma",
            )
        } else {
            (
                rgb_to_target_cmyk(
                    &page_rgb,
                    icc_profile_bytes.expect("CMYK ICC profile checked above"),
                )?,
                4,
                "/DeviceCMYK",
                "sRGB raster converted to target CMYK ICC with moxcms",
            )
        };
    let pdf_bytes = write_prepress_pdf(
        &plan,
        &image_bytes,
        channels,
        fallback_color_space,
        icc_profile_bytes,
    )?;
    let preflight = build_preflight_report(
        job,
        &plan,
        &pdf_bytes,
        channels,
        icc_profile_bytes,
        conversion_method,
    );
    if let Some(failed) = preflight
        .checks
        .iter()
        .find(|check| check.status == PlantPrepressPreflightStatus::Fail)
    {
        return Err(PlantPrepressExportError::new(format!(
            "plant preflight failed: {}",
            failed.detail
        )));
    }
    Ok(PlantPrepressPdf {
        plan,
        preflight,
        pdf_bytes,
    })
}

pub fn artwork_placements(
    template: &ExternalRecordPlantTemplateInput,
) -> Vec<PlantPrepressPlacement> {
    for layer in [GuideLayerKind::Bleed, GuideLayerKind::Trim] {
        let placements = guide_layer_placements(template, layer);
        if !placements.is_empty() {
            return placements;
        }
    }
    vec![rect_placement(template.document)]
}

pub fn target_dpi(job: &PlantPrepressJob) -> u16 {
    job.target
        .dpi
        .or(job.template.requirements.min_raster_ppi)
        .unwrap_or(DEFAULT_TARGET_DPI)
        .max(DEFAULT_TARGET_DPI)
}

pub fn mm_to_px(mm: f64, dpi: u16) -> u32 {
    ((mm.max(0.0) / MM_PER_INCH) * f64::from(dpi))
        .ceil()
        .max(1.0) as u32
}

pub fn mm_to_px_offset(mm: f64, dpi: u16) -> i64 {
    ((mm / MM_PER_INCH) * f64::from(dpi)).round() as i64
}

pub fn effective_ppi(info: &ArtworkInfo, placement: PlantPrepressPlacement) -> f64 {
    let width_in = placement.width_mm / MM_PER_INCH;
    let height_in = placement.height_mm / MM_PER_INCH;
    if !(width_in > 0.0 && height_in > 0.0) {
        return 0.0;
    }
    (f64::from(info.width_px) / width_in).min(f64::from(info.height_px) / height_in)
}

/// The files a plant-ready pack carries, in the order a plant reads them.
pub const PACK_PROOF_PDF_NAME: &str = "proof.pdf";
pub const PACK_PLANT_READY_PDF_NAME: &str = "plant-ready.pdf";
pub const PACK_SPEC_JSON_NAME: &str = "record-plant-spec.json";
pub const PACK_PREFLIGHT_MARKDOWN_NAME: &str = "preflight.md";
pub const PACK_README_NAME: &str = "README-for-plant.md";

/// Builds the pack's machine-readable spec.
///
/// A plant receives this alongside the PDFs, so it names every artifact in the
/// pack and repeats the geometry the production file was built against.
pub fn pack_spec_json(
    template: &ExternalRecordPlantTemplateInput,
    plan: &PlantPrepressPlan,
    preflight: &PlantPrepressPreflightReport,
) -> Result<String, PlantPrepressExportError> {
    let value = serde_json::json!({
        "packType": "bitneedle-record-plant-manufacturing-pack",
        "templateId": plan.template_id,
        "templateName": plan.template_name,
        "templateVersion": plan.template_version,
        "manufacturer": template.manufacturer,
        "product": template.product,
        "page": {
            "widthMm": plan.page_width_mm,
            "heightMm": plan.page_height_mm,
            "widthPx": plan.page_width_px,
            "heightPx": plan.page_height_px,
            "targetDpi": plan.target_dpi,
        },
        "color": {
            "mode": plan.color_mode,
            "iccProfile": plan.icc_profile,
            "outputConditionIdentifier": plan.output_condition_identifier,
            "sourceRgbProfile": plan.source_rgb_profile,
            "pdfStandard": plan.pdf_standard,
        },
        "artifacts": [
            {
                "path": PACK_PROOF_PDF_NAME,
                "role": "human-proof",
                "visibleGuides": true,
                "containsArtwork": true,
            },
            {
                "path": PACK_PLANT_READY_PDF_NAME,
                "role": "production",
                "visibleGuides": false,
                "containsArtwork": true,
            },
            {
                "path": PACK_SPEC_JSON_NAME,
                "role": "machine-readable-spec",
                "visibleGuides": false,
            },
            {
                "path": PACK_PREFLIGHT_MARKDOWN_NAME,
                "role": "human-readable-preflight",
                "visibleGuides": false,
            },
            {
                "path": PACK_README_NAME,
                "role": "plant-instructions",
                "visibleGuides": false,
            },
        ],
        "source": template.source,
        "sourceNotes": template.source_notes,
        "slots": plan.slots,
        "bleedAreas": plan.bleed_areas,
        "trimAreas": plan.trim_areas,
        "cutouts": plan.cutouts,
        "safetyAreas": plan.safety_areas,
        "preflight": preflight,
    });
    let json = serde_json::to_string_pretty(&value)
        .map_err(|error| PlantPrepressExportError::new(error.to_string()))?;
    Ok(format!("{json}\n"))
}

/// Renders the preflight report a person reads before approving the pack.
pub fn pack_preflight_markdown(
    plan: &PlantPrepressPlan,
    preflight: &PlantPrepressPreflightReport,
) -> String {
    let mut markdown = format!(
        "# Preflight\n\nTemplate: {}\nOutput condition: {}\nTarget: {} at {} DPI\n\n| Status | Check | Detail |\n| --- | --- | --- |\n",
        if plan.template_name.trim().is_empty() {
            "Selected plant template"
        } else {
            plan.template_name.trim()
        },
        plan.output_condition_identifier,
        plan.color_mode.to_uppercase(),
        plan.target_dpi,
    );
    for check in &preflight.checks {
        let summary = if check.summary.trim().is_empty() {
            if check.id.trim().is_empty() {
                "Check"
            } else {
                check.id.trim()
            }
        } else {
            check.summary.trim()
        };
        markdown.push_str(&format!(
            "| {} | {} | {} |\n",
            pack_preflight_status_label(check.status),
            pack_markdown_cell(summary),
            pack_markdown_cell(&check.detail),
        ));
    }
    markdown
}

/// Writes the plant's instructions. The one thing it must get across is that
/// the proof carries guides and only `plant-ready.pdf` goes on press.
pub fn pack_readme(plan: &PlantPrepressPlan) -> String {
    let color_mode = if plan.color_mode.trim().is_empty() {
        "CMYK".to_string()
    } else {
        plan.color_mode.to_uppercase()
    };
    format!(
        concat!(
            "# README for Plant\n",
            "\n",
            "Production file uses `{template}` geometry.\n",
            "\n",
            "- Proof file: `{proof}`, visible guides and calibration marks for human approval only.\n",
            "- Production file: `{production}`, artwork only with guide/template marks removed.\n",
            "- Page: {width_mm} x {height_mm} mm, {width_px} x {height_px} px at {dpi} DPI.\n",
            "- Color: {color}, output condition `{condition}`.\n",
            "- Slots: {slots} artwork placement(s).\n",
            "\n",
            "Do not print the proof guide layer. Use `{production}` for production.\n",
        ),
        template = if plan.template_name.trim().is_empty() {
            "selected plant template"
        } else {
            plan.template_name.trim()
        },
        proof = PACK_PROOF_PDF_NAME,
        production = PACK_PLANT_READY_PDF_NAME,
        width_mm = pack_fixed(plan.page_width_mm),
        height_mm = pack_fixed(plan.page_height_mm),
        width_px = plan.page_width_px,
        height_px = plan.page_height_px,
        dpi = plan.target_dpi,
        color = color_mode,
        condition = plan.output_condition_identifier,
        slots = plan.slots.len(),
    )
}

/// Wraps a rendered proof raster in a one-page PDF at its physical size.
///
/// The proof is the member a person signs off, so it has to open at the real
/// trim size in any PDF reader — a bare image carries no physical dimensions
/// and prints at whatever the reader guesses.
pub fn proof_pdf_from_jpeg(
    jpeg_bytes: &[u8],
    page_width_mm: f64,
    page_height_mm: f64,
    image_width: u32,
    image_height: u32,
    title: &str,
) -> Result<Vec<u8>, PlantPrepressExportError> {
    if jpeg_bytes.is_empty() {
        return Err(PlantPrepressExportError::new(
            "proof PDF needs rendered JPEG bytes",
        ));
    }
    if image_width == 0 || image_height == 0 {
        return Err(PlantPrepressExportError::new(
            "proof PDF needs the raster's pixel size",
        ));
    }
    if !page_width_mm.is_finite()
        || !page_height_mm.is_finite()
        || page_width_mm <= 0.0
        || page_height_mm <= 0.0
    {
        return Err(PlantPrepressExportError::new(
            "proof PDF needs a positive page size",
        ));
    }

    let width_pt = page_width_mm / MM_PER_INCH * POINTS_PER_INCH;
    let height_pt = page_height_mm / MM_PER_INCH * POINTS_PER_INCH;
    let content = format!("q\n{width_pt:.3} 0 0 {height_pt:.3} 0 0 cm\n/Im0 Do\nQ\n");

    let mut pdf: Vec<u8> = Vec::with_capacity(jpeg_bytes.len() + 1024);
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(6);

    let mut push = |pdf: &mut Vec<u8>, offsets: &mut Vec<usize>, text: &str| {
        offsets.push(pdf.len());
        pdf.extend_from_slice(text.as_bytes());
    };

    push(&mut pdf, &mut offsets, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    push(
        &mut pdf,
        &mut offsets,
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
    );
    push(
        &mut pdf,
        &mut offsets,
        &format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width_pt:.3} {height_pt:.3}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>\nendobj\n"
        ),
    );

    offsets.push(pdf.len());
    pdf.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {image_width} /Height {image_height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg_bytes.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(jpeg_bytes);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");

    push(
        &mut pdf,
        &mut offsets,
        &format!(
            "5 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
            content.len()
        ),
    );
    push(
        &mut pdf,
        &mut offsets,
        &format!(
            "6 0 obj\n<< /Title ({}) /Producer (Bitneedle Plant) >>\nendobj\n",
            escape_pdf_text(title)
        ),
    );

    let xref_offset = pdf.len();
    let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for offset in &offsets {
        xref.push_str(&format!("{offset:010} 00000 n \n"));
    }
    xref.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        offsets.len() + 1
    ));
    pdf.extend_from_slice(xref.as_bytes());
    Ok(pdf)
}

fn escape_pdf_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '(' | ')' => {
                escaped.push('\\');
                escaped.push(character);
            }
            // A raw newline inside a PDF string literal ends the object.
            '\r' | '\n' => escaped.push(' '),
            _ if character.is_ascii() => escaped.push(character),
            _ => escaped.push('?'),
        }
    }
    escaped
}

/// Packs the pack members into a store-only ZIP.
///
/// Store-only keeps this readable by every plant's unzip and keeps the writer
/// small enough to share with the Apple app, which has no zip writer of its own.
pub fn pack_zip_archive(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut local_parts: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut entries = 0u16;

    for (name, bytes) in files {
        let name_bytes = name.as_bytes();
        let crc = zip_crc32(bytes);
        let offset = local_parts.len() as u32;

        local_parts.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local_parts.extend_from_slice(&20u16.to_le_bytes());
        local_parts.extend_from_slice(&0u16.to_le_bytes());
        local_parts.extend_from_slice(&0u16.to_le_bytes()); // store, no compression
        local_parts.extend_from_slice(&0u16.to_le_bytes()); // no timestamps: byte-stable output
        local_parts.extend_from_slice(&0u16.to_le_bytes());
        local_parts.extend_from_slice(&crc.to_le_bytes());
        local_parts.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        local_parts.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        local_parts.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        local_parts.extend_from_slice(&0u16.to_le_bytes());
        local_parts.extend_from_slice(name_bytes);
        local_parts.extend_from_slice(bytes);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        central.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);

        entries = entries.saturating_add(1);
    }

    let central_offset = local_parts.len() as u32;
    let central_size = central.len() as u32;
    let mut archive = local_parts;
    archive.extend_from_slice(&central);
    archive.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&entries.to_le_bytes());
    archive.extend_from_slice(&entries.to_le_bytes());
    archive.extend_from_slice(&central_size.to_le_bytes());
    archive.extend_from_slice(&central_offset.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive
}

fn zip_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn pack_preflight_status_label(status: PlantPrepressPreflightStatus) -> &'static str {
    match status {
        PlantPrepressPreflightStatus::Pass => "pass",
        PlantPrepressPreflightStatus::Warn => "warn",
        PlantPrepressPreflightStatus::Fail => "fail",
    }
}

/// Trims trailing zeros the way the shipped pack does, so 106.000 reads as 106.
fn pack_fixed(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let text = format!("{value:.3}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Keeps a check's text inside one markdown table cell.
fn pack_markdown_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace('\n', " ")
        .replace('\r', " ")
}

fn normalized_color_mode(job: &PlantPrepressJob) -> String {
    job.target
        .color_mode
        .as_deref()
        .unwrap_or("CMYK")
        .trim()
        .to_ascii_lowercase()
}

fn normalized_source_rgb_profile(job: &PlantPrepressJob) -> String {
    let value = job
        .target
        .source_rgb_profile
        .as_deref()
        .unwrap_or("sRGB")
        .trim()
        .to_ascii_lowercase();
    match value.as_str() {
        "" | "srgb" | "s-rgb" | "s rgb" => "srgb".to_string(),
        other => other.to_string(),
    }
}

fn template_accepts_pdf(template: &ExternalRecordPlantTemplateInput) -> bool {
    template.requirements.accepted_formats.iter().any(|format| {
        let normalized = format.trim().to_ascii_lowercase();
        normalized == "pdf" || normalized.starts_with("pdf/")
    })
}

fn template_accepts_color_mode(
    template: &ExternalRecordPlantTemplateInput,
    color_mode: &str,
) -> bool {
    template
        .requirements
        .color_modes
        .iter()
        .any(|mode| normalize_color_requirement(mode) == color_mode)
}

fn normalize_color_requirement(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "cmyk" | "process cmyk" | "process color" | "process colour" => "cmyk".to_string(),
        "gray" | "grey" | "grayscale" | "greyscale" => "grayscale".to_string(),
        other => other.to_string(),
    }
}

fn normalized_pdf_standard(job: &PlantPrepressJob) -> String {
    job.target
        .pdf_standard
        .as_deref()
        .or(job.template.requirements.pdf_standard.as_deref())
        .unwrap_or("PDF/X-4")
        .trim()
        .to_ascii_lowercase()
}

#[derive(Debug, Clone)]
struct DecodedArtwork {
    slot_id: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl DecodedArtwork {
    fn from_asset(asset: &PlantPrepressArtworkBytes) -> Result<Self, PlantPrepressExportError> {
        let (image, source_icc_profile) = decode_artwork_rgba(&asset.slot_id, &asset.bytes)?;
        let (width, height) = image.dimensions();
        let mut rgba = image.into_raw();
        if let Some(source_icc_profile) = source_icc_profile.as_deref() {
            if !source_icc_profile.is_empty() {
                normalize_rgba_to_srgb(&asset.slot_id, &mut rgba, source_icc_profile)?;
            }
        }
        Ok(Self {
            slot_id: asset.slot_id.clone(),
            width,
            height,
            rgba,
        })
    }
}

fn decode_artwork_rgba(
    slot_id: &str,
    bytes: &[u8],
) -> Result<(RgbaImage, Option<Vec<u8>>), PlantPrepressExportError> {
    let format = image::guess_format(bytes).map_err(|error| {
        PlantPrepressExportError::new(format!(
            "failed to detect artwork format for slot {slot_id}: {error}",
        ))
    })?;
    match format {
        ImageFormat::Png => {
            let mut decoder =
                image::codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(|error| {
                    PlantPrepressExportError::new(format!(
                        "failed to decode PNG artwork slot {slot_id}: {error}",
                    ))
                })?;
            let icc_profile = decoder.icc_profile().map_err(|error| {
                PlantPrepressExportError::new(format!(
                    "failed to read PNG ICC profile for slot {slot_id}: {error}",
                ))
            })?;
            let image = DynamicImage::from_decoder(decoder).map_err(|error| {
                PlantPrepressExportError::new(format!(
                    "failed to decode PNG artwork slot {slot_id}: {error}",
                ))
            })?;
            Ok((image.to_rgba8(), icc_profile))
        }
        ImageFormat::Jpeg => {
            let mut decoder =
                image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(|error| {
                    PlantPrepressExportError::new(format!(
                        "failed to decode JPEG artwork slot {slot_id}: {error}",
                    ))
                })?;
            let icc_profile = decoder.icc_profile().map_err(|error| {
                PlantPrepressExportError::new(format!(
                    "failed to read JPEG ICC profile for slot {slot_id}: {error}",
                ))
            })?;
            let image = DynamicImage::from_decoder(decoder).map_err(|error| {
                PlantPrepressExportError::new(format!(
                    "failed to decode JPEG artwork slot {slot_id}: {error}",
                ))
            })?;
            Ok((image.to_rgba8(), icc_profile))
        }
        ImageFormat::WebP => {
            let mut decoder =
                image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|error| {
                    PlantPrepressExportError::new(format!(
                        "failed to decode WebP artwork slot {slot_id}: {error}",
                    ))
                })?;
            let icc_profile = decoder.icc_profile().map_err(|error| {
                PlantPrepressExportError::new(format!(
                    "failed to read WebP ICC profile for slot {slot_id}: {error}",
                ))
            })?;
            let image = DynamicImage::from_decoder(decoder).map_err(|error| {
                PlantPrepressExportError::new(format!(
                    "failed to decode WebP artwork slot {slot_id}: {error}",
                ))
            })?;
            Ok((image.to_rgba8(), icc_profile))
        }
        other => Err(PlantPrepressExportError::new(format!(
            "unsupported artwork format for slot {slot_id}: {other:?}",
        ))),
    }
}

fn rasterize_prepress_page(
    plan: &PlantPrepressPlan,
    artworks: &[DecodedArtwork],
) -> Result<Vec<u8>, PlantPrepressExportError> {
    let page_width = plan.page_width_px as usize;
    let page_height = plan.page_height_px as usize;
    let page_len = checked_image_len(page_width, page_height, 3, "prepress page")?;
    let mut page_rgb = vec![255; page_len];
    let artwork_by_slot = artworks
        .iter()
        .map(|artwork| (artwork.slot_id.as_str(), artwork))
        .collect::<HashMap<_, _>>();

    for slot in &plan.slots {
        let artwork = artwork_by_slot.get(slot.slot_id.as_str()).ok_or_else(|| {
            PlantPrepressExportError::new(format!(
                "missing artwork bytes for slot {}",
                slot.slot_id
            ))
        })?;
        let placement = slot.placement;
        let slot_width = mm_to_px(placement.width_mm, plan.target_dpi) as usize;
        let slot_height = mm_to_px(placement.height_mm, plan.target_dpi) as usize;
        let fitted = resize_cover_rgba(artwork, slot_width, slot_height)?;
        let x_offset = mm_to_px_offset(placement.x_mm, plan.target_dpi);
        let y_offset = mm_to_px_offset(placement.y_mm, plan.target_dpi);
        let radius_x = slot_width as f64 / 2.0;
        let radius_y = slot_height as f64 / 2.0;

        for slot_y in 0..slot_height {
            let page_y = y_offset + slot_y as i64;
            if page_y < 0 || page_y >= page_height as i64 {
                continue;
            }
            for slot_x in 0..slot_width {
                if placement.shape == PlantPrepressPlacementShape::Circle {
                    let dx = slot_x as f64 + 0.5 - radius_x;
                    let dy = slot_y as f64 + 0.5 - radius_y;
                    let distance =
                        (dx * dx) / (radius_x * radius_x) + (dy * dy) / (radius_y * radius_y);
                    if distance > 1.0 {
                        continue;
                    }
                }
                let page_x = x_offset + slot_x as i64;
                if page_x < 0 || page_x >= page_width as i64 {
                    continue;
                }
                let source_index = (slot_y * slot_width + slot_x) * 4;
                let alpha = fitted[source_index + 3] as u32;
                if alpha == 0 {
                    continue;
                }
                let page_index = ((page_y as usize * page_width) + page_x as usize) * 3;
                for channel in 0..3 {
                    let source = fitted[source_index + channel] as u32;
                    let target = page_rgb[page_index + channel] as u32;
                    page_rgb[page_index + channel] =
                        (((source * alpha) + (target * (255 - alpha)) + 127) / 255) as u8;
                }
            }
        }
    }

    mask_cutouts(&mut page_rgb, plan)?;
    Ok(page_rgb)
}

fn mask_cutouts(
    page_rgb: &mut [u8],
    plan: &PlantPrepressPlan,
) -> Result<(), PlantPrepressExportError> {
    if plan.cutouts.is_empty() {
        return Ok(());
    }
    let page_width = plan.page_width_px as usize;
    let page_height = plan.page_height_px as usize;
    for cutout in &plan.cutouts {
        let cutout_width = mm_to_px(cutout.width_mm, plan.target_dpi) as usize;
        let cutout_height = mm_to_px(cutout.height_mm, plan.target_dpi) as usize;
        let x_offset = mm_to_px_offset(cutout.x_mm, plan.target_dpi);
        let y_offset = mm_to_px_offset(cutout.y_mm, plan.target_dpi);
        let radius_x = cutout_width as f64 / 2.0;
        let radius_y = cutout_height as f64 / 2.0;

        for cutout_y in 0..cutout_height {
            let page_y = y_offset + cutout_y as i64;
            if page_y < 0 || page_y >= page_height as i64 {
                continue;
            }
            for cutout_x in 0..cutout_width {
                if cutout.shape == PlantPrepressPlacementShape::Circle {
                    let dx = cutout_x as f64 + 0.5 - radius_x;
                    let dy = cutout_y as f64 + 0.5 - radius_y;
                    let distance =
                        (dx * dx) / (radius_x * radius_x) + (dy * dy) / (radius_y * radius_y);
                    if distance > 1.0 {
                        continue;
                    }
                }
                let page_x = x_offset + cutout_x as i64;
                if page_x < 0 || page_x >= page_width as i64 {
                    continue;
                }
                let page_index = ((page_y as usize * page_width) + page_x as usize) * 3;
                page_rgb[page_index..page_index + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
    }
    Ok(())
}

fn resize_cover_rgba(
    artwork: &DecodedArtwork,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, PlantPrepressExportError> {
    if width == 0 || height == 0 || artwork.width == 0 || artwork.height == 0 {
        return Err(PlantPrepressExportError::new(
            "artwork or placement has zero size",
        ));
    }
    let source = RgbaImage::from_raw(artwork.width, artwork.height, artwork.rgba.clone())
        .ok_or_else(|| PlantPrepressExportError::new("decoded artwork RGBA buffer is invalid"))?;
    let scale = (width as f64 / artwork.width as f64).max(height as f64 / artwork.height as f64);
    let resized_width = ((artwork.width as f64 * scale).ceil() as u32).max(width as u32);
    let resized_height = ((artwork.height as f64 * scale).ceil() as u32).max(height as u32);
    let resized: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        image::imageops::resize(&source, resized_width, resized_height, FilterType::Lanczos3);
    let crop_x = ((resized_width as usize).saturating_sub(width)) / 2;
    let crop_y = ((resized_height as usize).saturating_sub(height)) / 2;
    let raw = resized.as_raw();
    let output_len = checked_image_len(width, height, 4, "resized artwork")?;
    let mut output = vec![0; output_len];
    let resized_stride = resized_width as usize * 4;
    let output_stride = width * 4;
    for row in 0..height {
        let source_start = ((crop_y + row) * resized_stride) + (crop_x * 4);
        let output_start = row * output_stride;
        output[output_start..output_start + output_stride]
            .copy_from_slice(&raw[source_start..source_start + output_stride]);
    }
    Ok(output)
}

fn checked_image_len(
    width: usize,
    height: usize,
    channels: usize,
    label: &str,
) -> Result<usize, PlantPrepressExportError> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| PlantPrepressExportError::new(format!("{label} is too large")))
}

fn rgb_to_grayscale(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .map(|pixel| {
            ((u32::from(pixel[0]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[2]) * 114)
                / 1000) as u8
        })
        .collect()
}

fn normalize_rgba_to_srgb(
    slot_id: &str,
    rgba: &mut Vec<u8>,
    source_icc_profile: &[u8],
) -> Result<(), PlantPrepressExportError> {
    let source_profile = ColorProfile::new_from_slice(source_icc_profile).map_err(|error| {
        PlantPrepressExportError::new(format!(
            "failed to parse source RGB ICC profile for slot {slot_id}: {error}",
        ))
    })?;
    if source_profile.color_space != DataColorSpace::Rgb {
        return Err(PlantPrepressExportError::new(format!(
            "source ICC profile for slot {slot_id} must be RGB, got {:?}",
            source_profile.color_space,
        )));
    }
    let target_profile = ColorProfile::new_srgb();
    let transform = source_profile
        .create_transform_8bit(
            Layout::Rgba,
            &target_profile,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|error| {
            PlantPrepressExportError::new(format!(
                "failed to create source RGB transform for slot {slot_id}: {error}",
            ))
        })?;
    let mut converted = vec![0; rgba.len()];
    transform.transform(rgba, &mut converted).map_err(|error| {
        PlantPrepressExportError::new(format!(
            "failed to convert source RGB profile for slot {slot_id}: {error}",
        ))
    })?;
    *rgba = converted;
    Ok(())
}

fn rgb_to_target_cmyk(
    rgb: &[u8],
    target_icc_profile: &[u8],
) -> Result<Vec<u8>, PlantPrepressExportError> {
    if rgb.len() % 3 != 0 {
        return Err(PlantPrepressExportError::new(
            "RGB prepress raster length is not divisible by 3",
        ));
    }
    let target_profile = ColorProfile::new_from_slice(target_icc_profile).map_err(|error| {
        PlantPrepressExportError::new(format!("failed to parse target CMYK ICC profile: {error}",))
    })?;
    if target_profile.color_space != DataColorSpace::Cmyk {
        return Err(PlantPrepressExportError::new(format!(
            "target ICC profile must be CMYK, got {:?}",
            target_profile.color_space,
        )));
    }
    let source_profile = ColorProfile::new_srgb();
    let transform = source_profile
        .create_transform_8bit(
            Layout::Rgb,
            &target_profile,
            Layout::Rgba,
            TransformOptions::default(),
        )
        .map_err(|error| {
            PlantPrepressExportError::new(format!(
                "failed to create sRGB to target CMYK transform: {error}",
            ))
        })?;
    let mut cmyk = vec![0; (rgb.len() / 3) * 4];
    transform.transform(rgb, &mut cmyk).map_err(|error| {
        PlantPrepressExportError::new(format!(
            "failed to convert sRGB raster to target CMYK: {error}",
        ))
    })?;
    Ok(cmyk)
}

fn build_preflight_report(
    job: &PlantPrepressJob,
    plan: &PlantPrepressPlan,
    pdf_bytes: &[u8],
    channels: u8,
    icc_profile_bytes: Option<&[u8]>,
    conversion_method: &str,
) -> PlantPrepressPreflightReport {
    let pdf_text = String::from_utf8_lossy(pdf_bytes);
    let mut checks = Vec::new();
    checks.push(preflight_check(
        "slot-count",
        PlantPrepressPreflightStatus::Pass,
        "Artwork slot count matches template placements",
        format!(
            "{} artwork slot(s) mapped to print placement(s).",
            plan.slots.len()
        ),
    ));
    checks.push(preflight_check(
        "image-resolution",
        PlantPrepressPreflightStatus::Pass,
        "Artwork resolution meets target DPI",
        format!(
            "Lowest effective resolution is {:.1} ppi for a {} ppi target.",
            plan.slots
                .iter()
                .map(|slot| slot.effective_ppi)
                .fold(f64::INFINITY, f64::min),
            plan.target_dpi,
        ),
    ));
    checks.push(preflight_check(
        "color-conversion",
        PlantPrepressPreflightStatus::Pass,
        "Final raster is converted through the exporter color pipeline",
        conversion_method.to_string(),
    ));
    checks.push(preflight_check(
        "output-intent",
        if icc_profile_bytes.is_some()
            && pdf_text.contains("/OutputIntents")
            && pdf_text.contains("/DestOutputProfile")
            && pdf_text.contains("/ICCBased")
        {
            PlantPrepressPreflightStatus::Pass
        } else if icc_profile_bytes.is_some() {
            PlantPrepressPreflightStatus::Fail
        } else {
            PlantPrepressPreflightStatus::Warn
        },
        if icc_profile_bytes.is_some()
            && pdf_text.contains("/OutputIntents")
            && pdf_text.contains("/DestOutputProfile")
            && pdf_text.contains("/ICCBased")
        {
            "PDF embeds an ICC output intent"
        } else if icc_profile_bytes.is_some() {
            "PDF is missing required ICC output intent structure"
        } else {
            "PDF has no ICC output intent"
        },
        if icc_profile_bytes.is_some()
            && pdf_text.contains("/OutputIntents")
            && pdf_text.contains("/DestOutputProfile")
            && pdf_text.contains("/ICCBased")
        {
            format!(
                "Embedded {} channel ICCBased image space and OutputIntent {}.",
                channels, plan.output_condition_identifier,
            )
        } else if icc_profile_bytes.is_some() {
            "ICC bytes were supplied, but the emitted PDF does not reference ICCBased image color and OutputIntent objects.".to_string()
        } else {
            "No ICC bytes were supplied for this grayscale export.".to_string()
        },
    ));
    checks.push(preflight_check(
        "pdf-header",
        if pdf_bytes.starts_with(b"%PDF-1.7") {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        "PDF header is present",
        if pdf_bytes.starts_with(b"%PDF-1.7") {
            "Emitted file starts with %PDF-1.7.".to_string()
        } else {
            "Emitted file does not start with the expected PDF header.".to_string()
        },
    ));
    let box_fragment = expected_pdf_box_fragment(plan);
    checks.push(preflight_check(
        "pdf-boxes",
        if pdf_text.contains(&box_fragment) {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        "PDF boxes match template document size",
        if pdf_text.contains(&box_fragment) {
            format!(
                "MediaBox, TrimBox, and BleedBox are {:.3} x {:.3} mm.",
                plan.page_width_mm, plan.page_height_mm,
            )
        } else {
            format!("Expected PDF page box fragment was not found: {box_fragment}")
        },
    ));
    let expected_image_size = format!(
        "/Width {} /Height {}",
        plan.page_width_px, plan.page_height_px
    );
    checks.push(preflight_check(
        "image-size",
        if pdf_text.contains(&expected_image_size) {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        "Embedded raster dimensions match target DPI",
        if pdf_text.contains(&expected_image_size) {
            format!(
                "Embedded raster is {} x {} px at {} ppi.",
                plan.page_width_px, plan.page_height_px, plan.target_dpi,
            )
        } else {
            format!("Expected image size fragment was not found: {expected_image_size}")
        },
    ));
    checks.push(preflight_check(
        "pdf-x-marker",
        if pdf_text.contains("/GTS_PDFXVersion")
            && pdf_text.contains("/S /GTS_PDFX")
            && pdf_text.contains("/Metadata")
            && pdf_text.contains("pdfxid:GTS_PDFXVersion")
        {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        "PDF/X markers and XMP metadata are present",
        if pdf_text.contains("/GTS_PDFXVersion")
            && pdf_text.contains("/S /GTS_PDFX")
            && pdf_text.contains("/Metadata")
            && pdf_text.contains("pdfxid:GTS_PDFXVersion")
        {
            format!("Emitted PDF declares {} in Info and XMP metadata.", plan.pdf_standard)
        } else {
            "Emitted PDF is missing one or more PDF/X marker, OutputIntent, or XMP metadata references.".to_string()
        },
    ));
    checks.push(preflight_check(
        "no-rgb-objects",
        if !pdf_text.contains("/DeviceRGB")
            && !pdf_text.contains("/CalRGB")
            && !pdf_text.contains("/Lab")
        {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        "No RGB PDF color objects are present",
        if !pdf_text.contains("/DeviceRGB")
            && !pdf_text.contains("/CalRGB")
            && !pdf_text.contains("/Lab")
        {
            "The emitted PDF object text contains no DeviceRGB, CalRGB, or Lab color spaces."
                .to_string()
        } else {
            "The emitted PDF object text contains an RGB/Lab color space marker.".to_string()
        },
    ));
    let leaked_guide_ids = job
        .template
        .guides
        .iter()
        .map(|guide| guide.id.trim())
        .filter(|id| !id.is_empty() && pdf_text.contains(*id))
        .collect::<Vec<_>>();
    checks.push(preflight_check(
        "template-guides",
        if leaked_guide_ids.is_empty() {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        "Template guides are not written to the final PDF layer",
        if leaked_guide_ids.is_empty() {
            "The final PDF contains one raster image XObject and no source guide object identifiers."
                .to_string()
        } else {
            format!("Guide identifiers leaked into emitted PDF: {}", leaked_guide_ids.join(", "))
        },
    ));
    checks.push(preflight_check(
        "cutouts",
        PlantPrepressPreflightStatus::Pass,
        "Cutout guide areas are masked in the final raster",
        if plan.cutouts.is_empty() {
            "Template contains no explicit no-print cutout guides.".to_string()
        } else {
            format!(
                "Masked {} cutout guide area(s) to no-print white.",
                plan.cutouts.len()
            )
        },
    ));
    checks.push(preflight_check(
        "safety-area",
        if plan.safety_areas.is_empty() {
            PlantPrepressPreflightStatus::Warn
        } else if plan.safety_clearance_confirmed {
            PlantPrepressPreflightStatus::Pass
        } else {
            PlantPrepressPreflightStatus::Fail
        },
        if plan.safety_areas.is_empty() {
            "No safety guide geometry was available"
        } else if plan.safety_clearance_confirmed {
            "Safety clearance was confirmed before export"
        } else {
            "Safety clearance was not confirmed"
        },
        if plan.safety_areas.is_empty() {
            "The exporter can omit guide layers, but cannot prove semantic text/logo clearance without safety guide geometry.".to_string()
        } else if plan.safety_clearance_confirmed {
            format!(
                "{} safety area(s) were confirmed clear before plant-ready export.",
                plan.safety_areas.len(),
            )
        } else {
            format!(
                "{} safety area(s) are present, but safetyClearance.confirmed was false.",
                plan.safety_areas.len(),
            )
        },
    ));
    PlantPrepressPreflightReport { checks }
}

fn preflight_check(
    id: impl Into<String>,
    status: PlantPrepressPreflightStatus,
    summary: impl Into<String>,
    detail: impl Into<String>,
) -> PlantPrepressPreflightCheck {
    PlantPrepressPreflightCheck {
        id: id.into(),
        status,
        summary: summary.into(),
        detail: detail.into(),
    }
}

fn expected_pdf_box_fragment(plan: &PlantPrepressPlan) -> String {
    let page_width_pt = pdf_number(mm_to_points(plan.page_width_mm));
    let page_height_pt = pdf_number(mm_to_points(plan.page_height_mm));
    format!(
        "/MediaBox [0 0 {page_width_pt} {page_height_pt}] /TrimBox [0 0 {page_width_pt} {page_height_pt}] /BleedBox [0 0 {page_width_pt} {page_height_pt}]"
    )
}

fn write_prepress_pdf(
    plan: &PlantPrepressPlan,
    image_bytes: &[u8],
    channels: u8,
    fallback_color_space: &str,
    icc_profile_bytes: Option<&[u8]>,
) -> Result<Vec<u8>, PlantPrepressExportError> {
    let compressed_image = zlib_compress(image_bytes, "prepress image")?;
    let compressed_icc = icc_profile_bytes
        .map(|bytes| zlib_compress(bytes, "ICC profile"))
        .transpose()?;
    let page_width_pt = mm_to_points(plan.page_width_mm);
    let page_height_pt = mm_to_points(plan.page_height_mm);
    let output_condition = escape_pdf_string(
        plan.output_condition_identifier
            .trim()
            .is_empty()
            .then_some("Device CMYK")
            .unwrap_or(plan.output_condition_identifier.trim()),
    );
    let title = escape_pdf_string(&format!("{} plant-ready artwork", plan.template_name));
    let xmp_metadata = xmp_metadata(plan);
    let content = format!(
        "q\n{} 0 0 {} 0 0 cm\n/Im0 Do\nQ\n",
        pdf_number(page_width_pt),
        pdf_number(page_height_pt),
    );
    let compressed_content = zlib_compress(content.as_bytes(), "PDF page content")?;
    let has_icc = compressed_icc.is_some();
    let color_space = if has_icc {
        "[/ICCBased 8 0 R]".to_string()
    } else {
        fallback_color_space.to_string()
    };
    let metadata_id = if has_icc { 9 } else { 7 };

    let mut pdf = PdfBuilder::new();
    let output_intents = if has_icc {
        " /OutputIntents [7 0 R]"
    } else {
        ""
    };
    pdf.object(
        1,
        &format!("<< /Type /Catalog /Pages 2 0 R{output_intents} /Metadata {metadata_id} 0 R >>"),
    );
    pdf.object(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    pdf.object(
        3,
        &format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /TrimBox [0 0 {} {}] /BleedBox [0 0 {} {}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>",
            pdf_number(page_width_pt),
            pdf_number(page_height_pt),
            pdf_number(page_width_pt),
            pdf_number(page_height_pt),
            pdf_number(page_width_pt),
            pdf_number(page_height_pt),
        ),
    );
    pdf.stream_object(
        4,
        &format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent 8 /Filter /FlateDecode",
            plan.page_width_px, plan.page_height_px, color_space,
        ),
        &compressed_image,
    );
    pdf.stream_object(5, "<< /Filter /FlateDecode", &compressed_content);
    pdf.object(
        6,
        &format!(
            "<< /Title ({title}) /Producer (Bitneedle record-prepress) /Creator (Bitneedle record-prepress) /GTS_PDFXVersion ({}) >>",
            escape_pdf_string(&plan.pdf_standard.to_uppercase()),
        ),
    );
    if let Some(compressed_icc) = compressed_icc {
        pdf.object(
            7,
            &format!(
                "<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier ({output_condition}) /Info ({output_condition}) /DestOutputProfile 8 0 R >>",
            ),
        );
        let alternate = match channels {
            1 => "/DeviceGray",
            4 => "/DeviceCMYK",
            _ => fallback_color_space,
        };
        pdf.stream_object(
            8,
            &format!(
                "<< /N {} /Alternate {} /Filter /FlateDecode",
                channels, alternate,
            ),
            &compressed_icc,
        );
    }
    pdf.stream_object(
        metadata_id,
        "<< /Type /Metadata /Subtype /XML",
        xmp_metadata.as_bytes(),
    );
    Ok(pdf.finish(1, 6))
}

fn xmp_metadata(plan: &PlantPrepressPlan) -> String {
    let title = escape_xml_text(&format!("{} plant-ready artwork", plan.template_name));
    let template_id = escape_xml_text(&plan.template_id);
    let template_version = escape_xml_text(plan.template_version.as_deref().unwrap_or(""));
    let pdf_standard = escape_xml_text(&plan.pdf_standard.to_uppercase());
    let output_condition = escape_xml_text(&plan.output_condition_identifier);
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:pdf="http://ns.adobe.com/pdf/1.3/" xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:bitneedle="https://bitneedle.com/ns/prepress/">
      <dc:title><rdf:Alt><rdf:li xml:lang="x-default">{title}</rdf:li></rdf:Alt></dc:title>
      <pdf:Producer>Bitneedle record-prepress</pdf:Producer>
      <xmp:CreatorTool>Bitneedle record-prepress</xmp:CreatorTool>
      <pdfxid:GTS_PDFXVersion>{pdf_standard}</pdfxid:GTS_PDFXVersion>
      <bitneedle:templateId>{template_id}</bitneedle:templateId>
      <bitneedle:templateVersion>{template_version}</bitneedle:templateVersion>
      <bitneedle:targetDpi>{}</bitneedle:targetDpi>
      <bitneedle:colorMode>{}</bitneedle:colorMode>
      <bitneedle:outputCondition>{output_condition}</bitneedle:outputCondition>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#,
        plan.target_dpi,
        escape_xml_text(&plan.color_mode),
    )
}

fn zlib_compress(bytes: &[u8], label: &str) -> Result<Vec<u8>, PlantPrepressExportError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).map_err(|error| {
        PlantPrepressExportError::new(format!("failed to compress {label}: {error}"))
    })?;
    encoder.finish().map_err(|error| {
        PlantPrepressExportError::new(format!("failed to finish {label} compression: {error}"))
    })
}

struct PdfBuilder {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl PdfBuilder {
    fn new() -> Self {
        Self {
            out: b"%PDF-1.7\n%\xFF\xFF\xFF\xFF\n".to_vec(),
            offsets: vec![0],
        }
    }

    fn object(&mut self, id: usize, body: &str) {
        self.begin_object(id);
        self.out.extend_from_slice(body.as_bytes());
        self.out.extend_from_slice(b"\nendobj\n");
    }

    fn stream_object(&mut self, id: usize, dict_prefix: &str, stream: &[u8]) {
        self.begin_object(id);
        self.out.extend_from_slice(dict_prefix.as_bytes());
        self.out
            .extend_from_slice(format!(" /Length {} >>\nstream\n", stream.len()).as_bytes());
        self.out.extend_from_slice(stream);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    fn begin_object(&mut self, id: usize) {
        if self.offsets.len() <= id {
            self.offsets.resize(id + 1, 0);
        }
        self.offsets[id] = self.out.len();
        self.out
            .extend_from_slice(format!("{id} 0 obj\n").as_bytes());
    }

    fn finish(mut self, root_id: usize, info_id: usize) -> Vec<u8> {
        let xref_offset = self.out.len();
        let object_count = self.offsets.len();
        self.out
            .extend_from_slice(format!("xref\n0 {object_count}\n").as_bytes());
        self.out.extend_from_slice(b"0000000000 65535 f \n");
        for id in 1..object_count {
            self.out
                .extend_from_slice(format!("{:010} 00000 n \n", self.offsets[id]).as_bytes());
        }
        self.out.extend_from_slice(
            format!(
                "trailer\n<< /Size {object_count} /Root {root_id} 0 R /Info {info_id} 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
            )
            .as_bytes(),
        );
        self.out
    }
}

fn mm_to_points(mm: f64) -> f64 {
    (mm / MM_PER_INCH) * POINTS_PER_INCH
}

fn pdf_number(value: f64) -> String {
    let rounded = (value * 10_000.0).round() / 10_000.0;
    if (rounded.fract()).abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn escape_pdf_string(value: &str) -> String {
    let mut out = String::new();
    for character in value.chars() {
        match character {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            character if character.is_control() => {}
            character if character.is_ascii() => out.push(character),
            _ => out.push('?'),
        }
    }
    out
}

fn escape_xml_text(value: &str) -> String {
    let mut out = String::new();
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' | '\r' | '\t' => out.push(' '),
            character if character.is_control() => {}
            character if character.is_ascii() => out.push(character),
            _ => out.push('?'),
        }
    }
    out
}

fn template_cutouts(template: &ExternalRecordPlantTemplateInput) -> Vec<PlantPrepressPlacement> {
    if matches!(
        template.kind,
        RecordPlantTemplateKind::CenterLabel | RecordPlantTemplateKind::PictureLabel
    ) {
        return Vec::new();
    }
    let mut cutouts = Vec::new();
    for layer in [GuideLayerKind::Hole, GuideLayerKind::Dink] {
        cutouts.extend(guide_layer_placements(template, layer));
    }
    sort_placements(&mut cutouts);
    cutouts
}

fn template_safety_areas(
    template: &ExternalRecordPlantTemplateInput,
) -> Vec<PlantPrepressPlacement> {
    guide_layer_placements(template, GuideLayerKind::Safety)
}

fn guide_layer_placements(
    template: &ExternalRecordPlantTemplateInput,
    layer: GuideLayerKind,
) -> Vec<PlantPrepressPlacement> {
    let mut placements: Vec<_> = template
        .guides
        .iter()
        .filter(|guide| guide.layer == layer)
        .filter_map(|guide| match guide.geometry {
            GuideGeometry::Circle { circle } => Some(PlantPrepressPlacement {
                x_mm: circle.cx - circle.radius,
                y_mm: circle.cy - circle.radius,
                width_mm: circle.diameter(),
                height_mm: circle.diameter(),
                shape: PlantPrepressPlacementShape::Circle,
            }),
            GuideGeometry::Rect { rect } => Some(rect_placement(rect)),
            _ => None,
        })
        .collect();
    sort_placements(&mut placements);
    placements
}

fn sort_placements(placements: &mut [PlantPrepressPlacement]) {
    placements.sort_by(|a, b| {
        a.y_mm
            .partial_cmp(&b.y_mm)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.x_mm
                    .partial_cmp(&b.x_mm)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
}

fn rect_placement(rect: RectMm) -> PlantPrepressPlacement {
    PlantPrepressPlacement {
        x_mm: rect.x,
        y_mm: rect.y,
        width_mm: rect.width,
        height_mm: rect.height,
        shape: PlantPrepressPlacementShape::Rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use record_plant::{
        external_record_plant_template_proof_bundle, MeasurementConfidence, OwnedPrintRequirements,
        OwnedSourceReference, RecordPlantProofMode, RecordPlantTemplateKind,
    };
    use std::path::{Path, PathBuf};

    fn two_up_job() -> PlantPrepressJob {
        PlantPrepressJob {
            template: ExternalRecordPlantTemplateInput {
                id: "test-two-up".to_string(),
                name: "Test two-up".to_string(),
                manufacturer: "Test Plant".to_string(),
                product: "7 in labels".to_string(),
                kind: RecordPlantTemplateKind::CenterLabel,
                version: Some("test".to_string()),
                document: RectMm::new(0.0, 0.0, 210.0, 100.0),
                confidence: MeasurementConfidence::PlantPublished,
                guides: vec![
                    record_plant::OwnedGuide {
                        id: "left-bleed".to_string(),
                        layer: GuideLayerKind::Bleed,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(50.0, 50.0, 48.0),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "right-bleed".to_string(),
                        layer: GuideLayerKind::Bleed,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(160.0, 50.0, 48.0),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "left-safety".to_string(),
                        layer: GuideLayerKind::Safety,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(50.0, 50.0, 44.0),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "right-safety".to_string(),
                        layer: GuideLayerKind::Safety,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(160.0, 50.0, 44.0),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "left-hole".to_string(),
                        layer: GuideLayerKind::Hole,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(50.0, 50.0, 3.5),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "right-hole".to_string(),
                        layer: GuideLayerKind::Hole,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(160.0, 50.0, 3.5),
                        },
                    },
                ],
                requirements: OwnedPrintRequirements {
                    preferred_output: "plant-ready PDF".to_string(),
                    accepted_formats: vec!["PDF".to_string()],
                    color_modes: vec!["CMYK".to_string(), "grayscale".to_string()],
                    min_raster_ppi: Some(300),
                    min_bitmap_ppi: None,
                    bleed_mm: Some(3.0),
                    safety_mm: Some(4.0),
                    keep_template_layer_out_of_final: true,
                    embed_or_outline_fonts: true,
                    pdf_standard: Some("PDF/X-4".to_string()),
                    output_condition_identifier: None,
                    notes: Vec::new(),
                },
                source: OwnedSourceReference {
                    title: "Fixture".to_string(),
                    url: "https://example.invalid/template".to_string(),
                    retrieved_on: "2026-06-04".to_string(),
                },
                source_notes: Vec::new(),
            },
            slots: vec![
                PlantPrepressArtworkSlot {
                    id: "side_a".to_string(),
                    artwork_path: "a.png".to_string(),
                    placement_index: None,
                },
                PlantPrepressArtworkSlot {
                    id: "side_b".to_string(),
                    artwork_path: "b.png".to_string(),
                    placement_index: None,
                },
            ],
            target: PlantPrepressTarget {
                dpi: Some(300),
                color_mode: Some("CMYK".to_string()),
                icc_profile: Some("/profiles/plant.icc".to_string()),
                source_rgb_profile: None,
                pdf_standard: Some("PDF/X-4".to_string()),
                output_condition_identifier: None,
            },
            safety_clearance: PlantPrepressSafetyClearance { confirmed: true },
        }
    }

    fn jungle_label_ab_job() -> PlantPrepressJob {
        PlantPrepressJob {
            template: ExternalRecordPlantTemplateInput {
                id: "test-jungle-12-label-ab-106mm".to_string(),
                name: "Jungle-style 12 in center label A/B".to_string(),
                manufacturer: "The Jungle Record Press".to_string(),
                product: "12 in center label".to_string(),
                kind: RecordPlantTemplateKind::CenterLabel,
                version: Some("golden".to_string()),
                document: RectMm::new(0.0, 0.0, 126.0, 232.0),
                confidence: MeasurementConfidence::DerivedFromPlantTemplate,
                guides: vec![
                    record_plant::OwnedGuide {
                        id: "label-a-bleed-diameter-106mm".to_string(),
                        layer: GuideLayerKind::Bleed,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(63.0, 63.0, 53.0),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "label-b-bleed-diameter-106mm".to_string(),
                        layer: GuideLayerKind::Bleed,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(63.0, 169.0, 53.0),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "label-a-trim-diameter-101mm".to_string(),
                        layer: GuideLayerKind::Trim,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(63.0, 63.0, 50.5),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "label-b-trim-diameter-101mm".to_string(),
                        layer: GuideLayerKind::Trim,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(63.0, 169.0, 50.5),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "label-a-center-hole-diameter-6p8mm".to_string(),
                        layer: GuideLayerKind::Hole,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(63.0, 63.0, 3.4),
                        },
                    },
                    record_plant::OwnedGuide {
                        id: "label-b-center-hole-diameter-6p8mm".to_string(),
                        layer: GuideLayerKind::Hole,
                        geometry: GuideGeometry::Circle {
                            circle: record_plant::CircleMm::new(63.0, 169.0, 3.4),
                        },
                    },
                ],
                requirements: OwnedPrintRequirements {
                    preferred_output: "PDF, TIFF, JPEG, or PSD".to_string(),
                    accepted_formats: vec!["PDF".to_string(), "TIFF".to_string()],
                    color_modes: vec!["CMYK".to_string()],
                    min_raster_ppi: Some(300),
                    min_bitmap_ppi: None,
                    bleed_mm: Some(2.5),
                    safety_mm: Some(3.2),
                    keep_template_layer_out_of_final: true,
                    embed_or_outline_fonts: false,
                    pdf_standard: Some("PDF/X-4".to_string()),
                    output_condition_identifier: Some("FOGRA39".to_string()),
                    notes: vec![
                        "Artwork must fit to the 106 mm edge and is trimmed to 101 mm.".to_string(),
                        "Center-hole rings are proof guides, not punched pixels in label art.".to_string(),
                    ],
                },
                source: OwnedSourceReference {
                    title: "Jungle 12inch Label Template A&B 106mm".to_string(),
                    url: "https://cdn.shopify.com/s/files/1/0699/2360/2699/files/12inch_Label_Template_A_B_106mm.pdf?v=1723155688".to_string(),
                    retrieved_on: "2026-06-05".to_string(),
                },
                source_notes: Vec::new(),
            },
            slots: vec![
                PlantPrepressArtworkSlot {
                    id: "side_a".to_string(),
                    artwork_path: "jungle-color-grid-source.png".to_string(),
                    placement_index: Some(0),
                },
                PlantPrepressArtworkSlot {
                    id: "side_b".to_string(),
                    artwork_path: "jungle-color-grid-source.png".to_string(),
                    placement_index: Some(1),
                },
            ],
            target: PlantPrepressTarget {
                dpi: Some(300),
                color_mode: Some("CMYK".to_string()),
                icc_profile: Some("FOGRA39.icc".to_string()),
                source_rgb_profile: None,
                pdf_standard: Some("PDF/X-4".to_string()),
                output_condition_identifier: Some("FOGRA39".to_string()),
            },
            safety_clearance: PlantPrepressSafetyClearance { confirmed: true },
        }
    }

    fn color_dpi_cmyk_job() -> PlantPrepressJob {
        PlantPrepressJob {
            template: ExternalRecordPlantTemplateInput {
                id: "test-color-dpi-cmyk-card".to_string(),
                name: "Test color and DPI card".to_string(),
                manufacturer: "Test Plant".to_string(),
                product: "1 in color card".to_string(),
                kind: RecordPlantTemplateKind::Insert,
                version: Some("golden".to_string()),
                document: RectMm::new(0.0, 0.0, 25.4, 25.4),
                confidence: MeasurementConfidence::PlantPublished,
                guides: Vec::new(),
                requirements: OwnedPrintRequirements {
                    preferred_output: "plant-ready PDF".to_string(),
                    accepted_formats: vec!["PDF".to_string()],
                    color_modes: vec!["CMYK".to_string()],
                    min_raster_ppi: Some(300),
                    min_bitmap_ppi: None,
                    bleed_mm: None,
                    safety_mm: None,
                    keep_template_layer_out_of_final: true,
                    embed_or_outline_fonts: true,
                    pdf_standard: Some("PDF/X-4".to_string()),
                    output_condition_identifier: Some("Generic CMYK".to_string()),
                    notes: vec![
                        "Synthetic 1 in fixture isolates raster DPI and CMYK conversion."
                            .to_string(),
                    ],
                },
                source: OwnedSourceReference {
                    title: "Synthetic color and DPI fixture".to_string(),
                    url: "https://example.invalid/prepress-color-dpi-cmyk".to_string(),
                    retrieved_on: "2026-06-05".to_string(),
                },
                source_notes: Vec::new(),
            },
            slots: vec![PlantPrepressArtworkSlot {
                id: "test_card".to_string(),
                artwork_path: "color-dpi-cmyk-source.png".to_string(),
                placement_index: None,
            }],
            target: PlantPrepressTarget {
                dpi: Some(300),
                color_mode: Some("CMYK".to_string()),
                icc_profile: Some("Generic CMYK Profile.icc".to_string()),
                source_rgb_profile: None,
                pdf_standard: Some("PDF/X-4".to_string()),
                output_condition_identifier: Some("Generic CMYK".to_string()),
            },
            safety_clearance: PlantPrepressSafetyClearance { confirmed: true },
        }
    }

    fn supplier_template_prepress_job(
        template: ExternalRecordPlantTemplateInput,
        artwork_path: &str,
    ) -> PlantPrepressJob {
        let slots = artwork_placements(&template)
            .iter()
            .enumerate()
            .map(|(index, _)| PlantPrepressArtworkSlot {
                id: slot_id_for_index(index),
                artwork_path: artwork_path.to_string(),
                placement_index: Some(index),
            })
            .collect();
        let output_condition_identifier = template.requirements.output_condition_identifier.clone();
        PlantPrepressJob {
            template,
            slots,
            target: PlantPrepressTarget {
                dpi: Some(300),
                color_mode: Some("CMYK".to_string()),
                icc_profile: Some("supplier-target.icc".to_string()),
                source_rgb_profile: None,
                pdf_standard: Some("PDF/X-4".to_string()),
                output_condition_identifier,
            },
            safety_clearance: PlantPrepressSafetyClearance { confirmed: true },
        }
    }

    fn slot_id_for_index(index: usize) -> String {
        if index < 26 {
            format!("side_{}", (b'a' + index as u8) as char)
        } else {
            format!("slot_{}", index + 1)
        }
    }

    #[test]
    fn validates_two_up_slots_without_duplication() {
        let job = two_up_job();
        let plan = build_prepress_plan(
            &job,
            &[
                ArtworkInfo {
                    slot_id: "side_a".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
                ArtworkInfo {
                    slot_id: "side_b".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
            ],
        )
        .unwrap();
        assert_eq!(plan.slots.len(), 2);
        assert_eq!(plan.template_version.as_deref(), Some("test"));
        assert_eq!(plan.slots[0].slot_id, "side_a");
        assert_eq!(plan.slots[0].placement_index, 0);
        assert_eq!(plan.slots[1].slot_id, "side_b");
        assert_eq!(plan.slots[1].placement_index, 1);
        assert_eq!(plan.bleed_areas.len(), 2);
        assert_eq!(plan.trim_areas.len(), 0);
        assert_eq!(plan.cutouts.len(), 0);
        assert_eq!(plan.safety_areas.len(), 2);
    }

    #[test]
    fn jungle_color_grid_proof_prep_matches_goldens() {
        let job = jungle_label_ab_job();
        let source_png = color_grid_label_png(1260, 1260);
        assert_png_golden(
            "jungle-color-grid-source.png",
            &source_png,
            GoldenPngMode::Rgba,
        );

        let artworks = [
            PlantPrepressArtworkBytes {
                slot_id: "side_a".to_string(),
                bytes: source_png.clone(),
            },
            PlantPrepressArtworkBytes {
                slot_id: "side_b".to_string(),
                bytes: source_png,
            },
        ];
        let decoded_artworks = artworks
            .iter()
            .map(DecodedArtwork::from_asset)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let artwork_infos = decoded_artworks
            .iter()
            .map(|artwork| ArtworkInfo {
                slot_id: artwork.slot_id.clone(),
                width_px: artwork.width,
                height_px: artwork.height,
            })
            .collect::<Vec<_>>();
        let plan = build_prepress_plan(&job, &artwork_infos).unwrap();
        assert_eq!(plan.page_width_px, mm_to_px(126.0, 300));
        assert_eq!(plan.page_height_px, mm_to_px(232.0, 300));
        assert_eq!(plan.slots.len(), 2);
        assert_eq!(plan.cutouts.len(), 0);

        let page_rgb = rasterize_prepress_page(&plan, &decoded_artworks).unwrap();
        assert_eq!(
            rgb_pixel_at_mm(&page_rgb, &plan, 63.0, 63.0),
            [16, 16, 16],
            "side A center-hole guide should not punch source label pixels to white",
        );
        assert_eq!(
            rgb_pixel_at_mm(&page_rgb, &plan, 63.0, 169.0),
            [16, 16, 16],
            "side B center-hole guide should not punch source label pixels to white",
        );
        assert_eq!(
            rgb_pixel_at_mm(&page_rgb, &plan, 120.0, 116.0),
            [255, 255, 255],
            "supplier page margin outside the label placements should stay white",
        );

        let prepped_png = rgb_png(&page_rgb, plan.page_width_px, plan.page_height_px);
        assert_png_golden(
            "jungle-color-grid-proof-prepped.png",
            &prepped_png,
            GoldenPngMode::Rgb,
        );
    }

    #[test]
    fn cached_supplier_alignment_proofs_match_goldens() {
        let artwork_png = color_grid_label_png(600, 600);
        assert_png_golden(
            "supplier-alignment-artwork.png",
            &artwork_png,
            GoldenPngMode::Rgba,
        );

        for (template_id, golden_name) in [
            (
                "the-jungle-record-press-12-label-ab-106mm-2023-10",
                "supplier-proof-jungle-12-label-ab.svg",
            ),
            (
                "united-record-pressing-7-large-hole-label-2025-08",
                "supplier-proof-urp-7-large-hole-2up.svg",
            ),
            (
                "memphis-record-pressing-7-center-label-gd17e-2019-01",
                "supplier-proof-memphis-7-single.svg",
            ),
            (
                "celebrate-12-picture-label",
                "supplier-proof-celebrate-12-picture-label.svg",
            ),
        ] {
            let template = cached_supplier_template(template_id);
            let proof_svg =
                artwork_alignment_proof_svg(&template, "supplier-alignment-artwork.png");
            assert!(proof_svg.contains(r#"data-layer="artwork""#));
            assert!(proof_svg.contains(r#"data-layer="process-swatch""#));
            assert!(proof_svg.contains(r#"data-layer="alignment-marker""#));
            assert!(proof_svg.contains(r#"data-layer="slot-label""#));
            assert!(proof_svg.contains(&format!(r#"data-record-plant-template="{}""#, template.id)));
            assert!(proof_svg.contains(&format!(r#"width="{:.3}mm""#, template.document.width)));
            assert!(proof_svg.contains(&format!(r#"height="{:.3}mm""#, template.document.height)));
            assert_text_golden(golden_name, &proof_svg);
        }
    }

    #[test]
    // Depends on a local label-artwork asset
    // (apps/site/web/assets/splash/records-photo-roll-avatar-bikini-label.png)
    // that is not committed to the repo, so it cannot run in a clean checkout.
    #[ignore]
    fn jungle_manufacturing_pack_artifacts_match_goldens() {
        let Some(icc_profile) = test_cmyk_icc_profile() else {
            eprintln!(
                "skipping Jungle pack PDF validation; set BITNEEDLE_TEST_CMYK_ICC to a CMYK ICC profile"
            );
            return;
        };

        let job = supplier_template_prepress_job(
            cached_supplier_template("the-jungle-record-press-12-label-ab-106mm-2023-10"),
            "jungle-pack-artwork-source.png",
        );
        let source_png = real_label_artwork_png();
        assert_png_golden(
            "jungle-pack-artwork-source.png",
            &source_png,
            GoldenPngMode::Rgba,
        );
        let artworks = job
            .slots
            .iter()
            .map(|slot| PlantPrepressArtworkBytes {
                slot_id: slot.id.clone(),
                bytes: source_png.clone(),
            })
            .collect::<Vec<_>>();
        let decoded_artworks = artworks
            .iter()
            .map(DecodedArtwork::from_asset)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let artwork_infos = decoded_artworks
            .iter()
            .map(|artwork| ArtworkInfo {
                slot_id: artwork.slot_id.clone(),
                width_px: artwork.width,
                height_px: artwork.height,
            })
            .collect::<Vec<_>>();
        let plan = build_prepress_plan(&job, &artwork_infos).unwrap();
        assert_eq!(
            plan.template_id,
            "the-jungle-record-press-12-label-ab-106mm-2023-10"
        );
        assert_eq!(plan.page_width_px, mm_to_px(126.0, 300));
        assert_eq!(plan.page_height_px, mm_to_px(232.0, 300));
        assert_eq!(plan.output_condition_identifier, "FOGRA39");
        assert_eq!(plan.slots.len(), 2);
        assert_eq!(plan.cutouts.len(), 0);

        let page_rgb = rasterize_prepress_page(&plan, &decoded_artworks).unwrap();
        let prepped_png = rgb_png(&page_rgb, plan.page_width_px, plan.page_height_px);
        assert_png_golden(
            "jungle-pack-plant-ready-raster.png",
            &prepped_png,
            GoldenPngMode::Rgb,
        );

        let proof_svg = page_raster_alignment_proof_svg(
            &job.template,
            "jungle-pack-plant-ready-raster.png",
            "Jungle A/B proof with artwork",
        );
        assert!(proof_svg.contains(r#"data-layer="proof-raster""#));
        assert!(proof_svg.contains(r#"data-layer="process-swatch""#));
        assert!(proof_svg.contains(r#"data-proof-mark="placement-center""#));
        assert!(proof_svg.contains(r#"data-slot-label="A""#));
        assert!(proof_svg.contains(r#"data-slot-label="B""#));
        assert!(proof_svg.contains("FOGRA39"));
        assert_text_golden("jungle-pack-proof.svg", &proof_svg);

        let output = export_prepress_pdf(&job, &artworks, Some(&icc_profile)).unwrap();
        let pdf_text = String::from_utf8_lossy(&output.pdf_bytes);
        assert!(output.pdf_bytes.starts_with(b"%PDF-1.7"));
        assert!(pdf_text.contains("/OutputIntents"));
        assert!(pdf_text.contains("/ICCBased"));
        assert!(pdf_text.contains("FOGRA39"));
        assert!(!pdf_text.contains("alignment-marker"));
        assert!(!pdf_text.contains("process-swatch"));
        assert!(!pdf_text.contains("slot-label"));
        assert!(!pdf_text.contains("label-a-bleed-diameter-106mm"));
        assert!(output
            .preflight
            .checks
            .iter()
            .all(|check| check.status != PlantPrepressPreflightStatus::Fail));

        assert_text_golden(
            "jungle-pack-record-plant-spec.json",
            &golden_pack_spec_json(&output.plan, &output.preflight),
        );
        assert_text_golden(
            "jungle-pack-preflight.md",
            &golden_preflight_markdown(&output.plan, &output.preflight),
        );
        assert_text_golden("jungle-pack-readme.md", &golden_pack_readme(&output.plan));
    }

    #[test]
    fn color_dpi_raster_and_cmyk_pipeline_matches_goldens() {
        let job = color_dpi_cmyk_job();
        let source_png = color_grid_label_png(300, 300);
        assert_png_golden(
            "color-dpi-cmyk-source.png",
            &source_png,
            GoldenPngMode::Rgba,
        );

        let artwork = PlantPrepressArtworkBytes {
            slot_id: "test_card".to_string(),
            bytes: source_png.clone(),
        };
        let decoded_artwork = DecodedArtwork::from_asset(&artwork).unwrap();
        let plan = build_prepress_plan(
            &job,
            &[ArtworkInfo {
                slot_id: decoded_artwork.slot_id.clone(),
                width_px: decoded_artwork.width,
                height_px: decoded_artwork.height,
            }],
        )
        .unwrap();
        assert_eq!(plan.target_dpi, 300);
        assert_eq!(plan.page_width_px, 300);
        assert_eq!(plan.page_height_px, 300);
        assert_eq!(plan.color_mode, "cmyk");
        assert_eq!(plan.output_condition_identifier, "Generic CMYK");
        assert_eq!(plan.slots.len(), 1);
        assert!(
            (plan.slots[0].effective_ppi - 300.0).abs() < 1e-9,
            "one-inch 300 px source should place at exactly 300 ppi, got {}",
            plan.slots[0].effective_ppi,
        );

        let page_rgb = rasterize_prepress_page(&plan, &[decoded_artwork]).unwrap();
        assert_eq!(page_rgb.len(), 300 * 300 * 3);
        let source_rgb = png_pixels(&source_png, GoldenPngMode::Rgb);
        assert_eq!(source_rgb.0, 300);
        assert_eq!(source_rgb.1, 300);
        assert_eq!(
            page_rgb, source_rgb.2,
            "300 DPI no-scale raster path should preserve source RGB pixels",
        );

        let raster_png = rgb_png(&page_rgb, plan.page_width_px, plan.page_height_px);
        assert_png_golden(
            "color-dpi-cmyk-raster-rgb.png",
            &raster_png,
            GoldenPngMode::Rgb,
        );

        let Some((profile_path, icc_profile)) = test_cmyk_icc_profile_with_path() else {
            eprintln!(
                "skipping CMYK golden section; set BITNEEDLE_TEST_CMYK_ICC to a CMYK ICC profile"
            );
            return;
        };
        let profile_fingerprint =
            format!("fingerprint={}\n", cmyk_profile_fingerprint(&icc_profile));
        if !golden_text_matches_or_update("color-dpi-cmyk-profile.txt", &profile_fingerprint) {
            eprintln!(
                "skipping CMYK golden section; {} does not match color-dpi-cmyk-profile.txt",
                profile_path,
            );
            return;
        }

        let cmyk = rgb_to_target_cmyk(&page_rgb, &icc_profile).unwrap();
        assert_eq!(cmyk.len(), 300 * 300 * 4);
        let white = cmyk_pixel_at(&cmyk, plan.page_width_px, 25, 25);
        let cyan = cmyk_pixel_at(&cmyk, plan.page_width_px, 75, 75);
        let magenta = cmyk_pixel_at(&cmyk, plan.page_width_px, 125, 75);
        let yellow = cmyk_pixel_at(&cmyk, plan.page_width_px, 175, 75);
        let black = cmyk_pixel_at(&cmyk, plan.page_width_px, 275, 275);
        assert!(
            white.iter().all(|channel| *channel <= 2),
            "white source swatch should stay near zero process ink, got {white:?}",
        );
        assert!(
            cyan[0] > 128,
            "cyan source swatch should drive C, got {cyan:?}"
        );
        assert!(
            magenta[1] > 128,
            "magenta source swatch should drive M, got {magenta:?}",
        );
        assert!(
            yellow[2] > 128,
            "yellow source swatch should drive Y, got {yellow:?}",
        );
        assert!(
            black.iter().any(|channel| *channel > 128),
            "black source swatch should carry heavy ink, got {black:?}",
        );

        assert_bytes_golden("color-dpi-cmyk-output.cmyk", &cmyk);
        let channel_preview_png =
            cmyk_channel_preview_png(&cmyk, plan.page_width_px, plan.page_height_px);
        assert_png_golden(
            "color-dpi-cmyk-channel-preview.png",
            &channel_preview_png,
            GoldenPngMode::Rgb,
        );
    }

    #[test]
    fn rejects_single_artwork_for_two_up_template() {
        let mut job = two_up_job();
        job.slots.truncate(1);
        let error = build_prepress_plan(
            &job,
            &[ArtworkInfo {
                slot_id: "side_a".to_string(),
                width_px: 1200,
                height_px: 1200,
            }],
        )
        .unwrap_err();
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("template has 2 print slots")));
    }

    #[test]
    fn rejects_missing_cmyk_icc_profile() {
        let mut job = two_up_job();
        job.target.icc_profile = None;
        let error = build_prepress_plan(
            &job,
            &[
                ArtworkInfo {
                    slot_id: "side_a".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
                ArtworkInfo {
                    slot_id: "side_b".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
            ],
        )
        .unwrap_err();
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("requires target.iccProfile")));
    }

    #[test]
    fn rejects_low_effective_ppi() {
        let job = two_up_job();
        let error = build_prepress_plan(
            &job,
            &[
                ArtworkInfo {
                    slot_id: "side_a".to_string(),
                    width_px: 600,
                    height_px: 600,
                },
                ArtworkInfo {
                    slot_id: "side_b".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
            ],
        )
        .unwrap_err();
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("below target 300 ppi")));
    }

    #[test]
    fn rejects_unconfirmed_safety_clearance() {
        let mut job = two_up_job();
        job.safety_clearance.confirmed = false;
        let error = build_prepress_plan(
            &job,
            &[
                ArtworkInfo {
                    slot_id: "side_a".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
                ArtworkInfo {
                    slot_id: "side_b".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
            ],
        )
        .unwrap_err();
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("safetyClearance.confirmed")));
    }

    #[test]
    fn rejects_missing_known_plant_requirements() {
        let mut job = two_up_job();
        job.template.requirements.accepted_formats = vec!["TIFF".to_string()];
        job.template.requirements.keep_template_layer_out_of_final = false;
        job.target.color_mode = Some("RGB".to_string());
        let error = build_prepress_plan(
            &job,
            &[
                ArtworkInfo {
                    slot_id: "side_a".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
                ArtworkInfo {
                    slot_id: "side_b".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
            ],
        )
        .unwrap_err();
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("accepts PDF output")));
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("template/guide layers")));
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.contains("colorMode must be CMYK or grayscale")));
    }

    #[test]
    fn exports_cmyk_pdf_without_template_guides() {
        let Some(icc_profile) = test_cmyk_icc_profile() else {
            eprintln!(
                "skipping CMYK export test; set BITNEEDLE_TEST_CMYK_ICC to a CMYK ICC profile"
            );
            return;
        };
        let job = two_up_job();
        let output = export_prepress_pdf(&job, &two_slot_artworks(), Some(&icc_profile)).unwrap();
        assert_eq!(output.plan.slots.len(), 2);
        assert!(output.pdf_bytes.starts_with(b"%PDF-1.7"));
        assert!(output
            .preflight
            .checks
            .iter()
            .any(|check| check.id == "color-conversion"
                && check.status == PlantPrepressPreflightStatus::Pass
                && check.detail.contains("moxcms")));
        let pdf_text = String::from_utf8_lossy(&output.pdf_bytes);
        assert!(pdf_text.contains("/ICCBased 8 0 R"));
        assert!(pdf_text.contains("/OutputIntents [7 0 R]"));
        assert!(pdf_text.contains("/Metadata 9 0 R"));
        assert!(pdf_text.contains("pdfxid:GTS_PDFXVersion"));
        assert!(!pdf_text.contains("left-bleed"));
        assert!(!pdf_text.contains("right-bleed"));
        assert!(!pdf_text.contains("left-hole"));
        assert!(!pdf_text.contains("right-hole"));
    }

    #[test]
    fn exports_grayscale_pdf_with_output_intent() {
        let mut job = two_up_job();
        job.target.color_mode = Some("grayscale".to_string());
        job.target.icc_profile = Some("generic-gray-gamma-2.2.icc".to_string());
        job.target.output_condition_identifier = Some("Generic Gray Gamma 2.2".to_string());
        let gray_profile = ColorProfile::new_gray_with_gamma(2.2).encode().unwrap();
        let output = export_prepress_pdf(&job, &two_slot_artworks(), Some(&gray_profile)).unwrap();
        assert_eq!(output.plan.color_mode, "grayscale");
        assert!(output
            .preflight
            .checks
            .iter()
            .any(|check| check.id == "output-intent"
                && check.status == PlantPrepressPreflightStatus::Pass));
        let pdf_text = String::from_utf8_lossy(&output.pdf_bytes);
        assert!(pdf_text.contains("/ICCBased 8 0 R"));
        assert!(pdf_text.contains("/N 1 /Alternate /DeviceGray"));
        assert!(!pdf_text.contains("/DeviceRGB"));
    }

    #[test]
    fn export_rejects_missing_cmyk_icc_profile_bytes() {
        let job = two_up_job();
        let error = export_prepress_pdf(
            &job,
            &[
                PlantPrepressArtworkBytes {
                    slot_id: "side_a".to_string(),
                    bytes: solid_rgba_png(1200, 1200, [240, 20, 40, 255]),
                },
                PlantPrepressArtworkBytes {
                    slot_id: "side_b".to_string(),
                    bytes: solid_rgba_png(1200, 1200, [20, 80, 240, 255]),
                },
            ],
            None,
        )
        .unwrap_err();
        assert!(error.message().contains("requires ICC profile bytes"));
    }

    #[test]
    fn export_rejects_invalid_cmyk_icc_profile_bytes() {
        let job = two_up_job();
        let error = export_prepress_pdf(&job, &two_slot_artworks(), Some(b"test-cmyk-icc-profile"))
            .unwrap_err();
        assert!(error
            .message()
            .contains("failed to parse target CMYK ICC profile"));
    }

    #[test]
    fn export_rejects_rgb_icc_profile_as_cmyk_target() {
        let job = two_up_job();
        let rgb_profile = ColorProfile::new_srgb().encode().unwrap();
        let error =
            export_prepress_pdf(&job, &two_slot_artworks(), Some(&rgb_profile)).unwrap_err();
        assert!(error.message().contains("target ICC profile must be CMYK"));
    }

    #[test]
    fn export_rejects_single_artwork_for_two_up_template() {
        let mut job = two_up_job();
        job.slots.truncate(1);
        let error = export_prepress_pdf(
            &job,
            &[PlantPrepressArtworkBytes {
                slot_id: "side_a".to_string(),
                bytes: solid_rgba_png(1200, 1200, [240, 20, 40, 255]),
            }],
            None,
        )
        .unwrap_err();
        assert!(error.message().contains("template has 2 print slots"));
    }

    fn two_slot_artworks() -> Vec<PlantPrepressArtworkBytes> {
        vec![
            PlantPrepressArtworkBytes {
                slot_id: "side_a".to_string(),
                bytes: solid_rgba_png(1200, 1200, [240, 20, 40, 255]),
            },
            PlantPrepressArtworkBytes {
                slot_id: "side_b".to_string(),
                bytes: solid_rgba_png(1200, 1200, [20, 80, 240, 255]),
            },
        ]
    }

    fn test_cmyk_icc_profile() -> Option<Vec<u8>> {
        test_cmyk_icc_profile_with_path().map(|(_, bytes)| bytes)
    }

    fn test_cmyk_icc_profile_with_path() -> Option<(String, Vec<u8>)> {
        let mut candidates = Vec::new();
        if let Ok(path) = std::env::var("BITNEEDLE_TEST_CMYK_ICC") {
            candidates.push(path);
        }
        candidates.push("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc".to_string());
        for path in candidates {
            if let Ok(bytes) = std::fs::read(&path) {
                if ColorProfile::new_from_slice(&bytes)
                    .map(|profile| profile.color_space == DataColorSpace::Cmyk)
                    .unwrap_or(false)
                {
                    return Some((path, bytes));
                }
            }
        }
        None
    }

    #[derive(Debug, Clone, Copy)]
    enum GoldenPngMode {
        Rgb,
        Rgba,
    }

    fn color_grid_label_png(width: u32, height: u32) -> Vec<u8> {
        let swatches = [
            [255, 255, 255],
            [229, 229, 229],
            [128, 128, 128],
            [16, 16, 16],
            [237, 28, 36],
            [0, 166, 81],
            [0, 114, 188],
            [0, 174, 239],
            [236, 0, 140],
            [255, 242, 0],
            [247, 148, 29],
            [102, 45, 145],
            [128, 0, 0],
            [0, 96, 64],
            [0, 52, 102],
            [0, 128, 128],
            [96, 57, 19],
            [194, 24, 91],
            [244, 204, 204],
            [243, 225, 184],
            [176, 223, 229],
            [192, 224, 160],
            [214, 176, 224],
            [242, 160, 120],
            [255, 128, 128],
            [128, 255, 128],
            [128, 128, 255],
            [128, 255, 255],
            [255, 128, 255],
            [255, 255, 128],
            [65, 105, 225],
            [46, 139, 87],
            [220, 20, 60],
            [255, 215, 0],
            [75, 0, 130],
            [0, 0, 0],
        ];
        let columns = 6;
        let rows = 6;
        let width_usize = width as usize;
        let height_usize = height as usize;
        let mut rgba = vec![255; width_usize * height_usize * 4];
        for y in 0..height {
            for x in 0..width {
                let column = ((x as usize * columns) / width_usize).min(columns - 1);
                let row = ((y as usize * rows) / height_usize).min(rows - 1);
                let mut rgb = swatches[row * columns + column];
                let on_grid_line = (x as usize * columns) % width_usize < 4
                    || (y as usize * rows) % height_usize < 4;
                if on_grid_line {
                    rgb = [12, 12, 12];
                }
                let index = ((y as usize * width_usize) + x as usize) * 4;
                rgba[index..index + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }

        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;
        let crosshair_half = (width.min(height) as f64 * 0.12).round() as i64;
        let ring_inner = width.min(height) as f64 * 0.026;
        let ring_outer = width.min(height) as f64 * 0.036;
        for y in 0..height {
            for x in 0..width {
                let dx = x as f64 + 0.5 - center_x;
                let dy = y as f64 + 0.5 - center_y;
                let distance = (dx * dx + dy * dy).sqrt();
                let on_crosshair = (dx.abs() <= 3.0 && dy.abs() <= crosshair_half as f64)
                    || (dy.abs() <= 3.0 && dx.abs() <= crosshair_half as f64);
                let color = if distance <= ring_inner {
                    Some([16, 16, 16])
                } else if distance <= ring_outer {
                    Some([236, 0, 140])
                } else if on_crosshair {
                    Some([16, 16, 16])
                } else {
                    None
                };
                if let Some(rgb) = color {
                    let index = ((y as usize * width_usize) + x as usize) * 4;
                    rgba[index..index + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        }

        rgba_png(&rgba, width, height)
    }

    fn real_label_artwork_png() -> Vec<u8> {
        std::fs::read(
            repo_root()
                .join("apps/site/web/assets/splash/records-photo-roll-avatar-bikini-label.png"),
        )
        .unwrap()
    }

    fn rgb_pixel_at_mm(rgb: &[u8], plan: &PlantPrepressPlan, x_mm: f64, y_mm: f64) -> [u8; 3] {
        let x = mm_to_px_offset(x_mm, plan.target_dpi)
            .clamp(0, i64::from(plan.page_width_px.saturating_sub(1))) as usize;
        let y = mm_to_px_offset(y_mm, plan.target_dpi)
            .clamp(0, i64::from(plan.page_height_px.saturating_sub(1))) as usize;
        let index = ((y * plan.page_width_px as usize) + x) * 3;
        [rgb[index], rgb[index + 1], rgb[index + 2]]
    }

    fn cmyk_pixel_at(cmyk: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y as usize * width as usize) + x as usize) * 4;
        [
            cmyk[index],
            cmyk[index + 1],
            cmyk[index + 2],
            cmyk[index + 3],
        ]
    }

    fn cmyk_channel_preview_png(cmyk: &[u8], width: u32, height: u32) -> Vec<u8> {
        assert_eq!(cmyk.len(), width as usize * height as usize * 4);
        let out_width = width * 2;
        let out_height = height * 2;
        let mut rgb = vec![255; out_width as usize * out_height as usize * 3];

        for y in 0..height {
            for x in 0..width {
                let source_index = ((y as usize * width as usize) + x as usize) * 4;
                let c = cmyk[source_index];
                let m = cmyk[source_index + 1];
                let ink_y = cmyk[source_index + 2];
                let k = cmyk[source_index + 3];
                let quadrants = [
                    (x, y, [255u8.saturating_sub(c), 255, 255]),
                    (x + width, y, [255, 255u8.saturating_sub(m), 255]),
                    (x, y + height, [255, 255, 255u8.saturating_sub(ink_y)]),
                    (
                        x + width,
                        y + height,
                        [
                            255u8.saturating_sub(k),
                            255u8.saturating_sub(k),
                            255u8.saturating_sub(k),
                        ],
                    ),
                ];
                for (out_x, out_y, pixel) in quadrants {
                    let target_index = ((out_y as usize * out_width as usize) + out_x as usize) * 3;
                    rgb[target_index..target_index + 3].copy_from_slice(&pixel);
                }
            }
        }

        rgb_png(&rgb, out_width, out_height)
    }

    fn cached_supplier_template(id: &str) -> ExternalRecordPlantTemplateInput {
        // Read the registry through the crate that owns it, so these tests
        // exercise the same templates a plant is actually sent.
        let template = record_plant::plant_template(id)
            .unwrap_or_else(|| panic!("missing cached supplier template {id}"));
        let json = serde_json::to_string(template).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    fn artwork_alignment_proof_svg(
        template: &ExternalRecordPlantTemplateInput,
        artwork_href: &str,
    ) -> String {
        let bundle =
            external_record_plant_template_proof_bundle(template, RecordPlantProofMode::Proof)
                .unwrap();
        let overlay = proof_overlay_body(&bundle.guide_svg);
        let doc = template.document;
        let placements = artwork_placements(template);
        let mut svg = String::new();
        svg.push_str(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.3}mm" height="{:.3}mm" viewBox="0 0 {:.3} {:.3}" data-record-plant-template="{}" data-record-plant-proof-mode="proof" data-visible-guides="true" data-artwork-proof="true">"#,
            doc.width,
            doc.height,
            doc.width,
            doc.height,
            escape_xml_text(template.id.trim()),
        ));
        svg.push_str(&format!(
            "<title>{} artwork alignment proof</title>",
            escape_xml_text(&template.name),
        ));
        svg.push_str(&format!(
            "<desc>Artwork laid onto {} supplier dimensions with visible proof guides.</desc>",
            escape_xml_text(&template.manufacturer),
        ));
        svg.push_str(r#"<rect x="0" y="0" width="100%" height="100%" fill="white"/>"#);
        svg.push_str("<defs>");
        for (index, placement) in placements.iter().enumerate() {
            match placement.shape {
                PlantPrepressPlacementShape::Circle => {
                    svg.push_str(&format!(
                        r#"<clipPath id="artwork-placement-clip-{index}"><circle cx="{:.3}" cy="{:.3}" r="{:.3}"/></clipPath>"#,
                        placement.x_mm + placement.width_mm / 2.0,
                        placement.y_mm + placement.height_mm / 2.0,
                        placement.width_mm.min(placement.height_mm) / 2.0,
                    ));
                }
                PlantPrepressPlacementShape::Rect => {
                    svg.push_str(&format!(
                        r#"<clipPath id="artwork-placement-clip-{index}"><rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}"/></clipPath>"#,
                        placement.x_mm,
                        placement.y_mm,
                        placement.width_mm,
                        placement.height_mm,
                    ));
                }
            }
        }
        svg.push_str("</defs>");
        svg.push_str(r#"<g data-layer="artwork">"#);
        for (index, placement) in placements.iter().enumerate() {
            svg.push_str(&format!(
                r#"<image data-slot-index="{index}" href="{}" x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" preserveAspectRatio="xMidYMid slice" clip-path="url(#artwork-placement-clip-{index})"/>"#,
                escape_xml_text(artwork_href),
                placement.x_mm,
                placement.y_mm,
                placement.width_mm,
                placement.height_mm,
            ));
        }
        svg.push_str("</g>");
        svg.push_str(overlay);
        svg.push_str("</svg>");
        svg
    }

    fn page_raster_alignment_proof_svg(
        template: &ExternalRecordPlantTemplateInput,
        raster_href: &str,
        title: &str,
    ) -> String {
        let bundle =
            external_record_plant_template_proof_bundle(template, RecordPlantProofMode::Proof)
                .unwrap();
        let overlay = proof_overlay_body(&bundle.guide_svg);
        let doc = template.document;
        format!(
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.3}mm" height="{height:.3}mm" viewBox="0 0 {width:.3} {height:.3}" data-record-plant-template="{template_id}" data-record-plant-proof-mode="proof" data-visible-guides="true" data-artwork-proof="true">"#,
                "<title>{title}</title>",
                "<desc>Plant-ready raster laid under visible supplier proof guides.</desc>",
                r#"<rect x="0" y="0" width="100%" height="100%" fill="white"/>"#,
                r#"<image data-layer="proof-raster" href="{raster_href}" x="0" y="0" width="{width:.3}" height="{height:.3}" preserveAspectRatio="none"/>"#,
                "{overlay}",
                "</svg>"
            ),
            width = doc.width,
            height = doc.height,
            template_id = escape_xml_text(template.id.trim()),
            title = escape_xml_text(title),
            raster_href = escape_xml_text(raster_href),
            overlay = overlay,
        )
    }

    fn proof_overlay_body(guide_svg: &str) -> &str {
        let page_rect = r#"<rect x="0" y="0" width="100%" height="100%" fill="white"/>"#;
        let start = guide_svg
            .find(page_rect)
            .map(|index| index + page_rect.len())
            .unwrap_or_else(|| guide_svg.find('>').map(|index| index + 1).unwrap_or(0));
        let end = guide_svg.rfind("</svg>").unwrap_or(guide_svg.len());
        &guide_svg[start..end]
    }

    /// The pack members the committed goldens were blessed against, before the
    /// pack builders moved to this crate's public API. The proof member here is
    /// an SVG; the shipped pack sends a PDF.
    fn golden_pack_spec_json(
        plan: &PlantPrepressPlan,
        preflight: &PlantPrepressPreflightReport,
    ) -> String {
        let value = serde_json::json!({
            "packType": "bitneedle-record-plant-manufacturing-pack",
            "templateId": plan.template_id,
            "templateName": plan.template_name,
            "templateVersion": plan.template_version,
            "page": {
                "widthMm": plan.page_width_mm,
                "heightMm": plan.page_height_mm,
                "widthPx": plan.page_width_px,
                "heightPx": plan.page_height_px,
                "targetDpi": plan.target_dpi,
            },
            "color": {
                "mode": plan.color_mode,
                "iccProfile": plan.icc_profile,
                "outputConditionIdentifier": plan.output_condition_identifier,
                "sourceRgbProfile": plan.source_rgb_profile,
                "pdfStandard": plan.pdf_standard,
            },
            "artifacts": [
                {
                    "path": "proof.svg",
                    "role": "human-proof",
                    "visibleGuides": true,
                    "containsArtwork": true,
                },
                {
                    "path": "plant-ready.pdf",
                    "role": "production",
                    "visibleGuides": false,
                    "containsArtwork": true,
                    "goldenCommitted": false,
                    "validation": "validated in memory by Rust test",
                },
                {
                    "path": "record-plant-spec.json",
                    "role": "machine-readable-spec",
                    "visibleGuides": false,
                },
                {
                    "path": "preflight.md",
                    "role": "human-readable-preflight",
                    "visibleGuides": false,
                },
                {
                    "path": "README-for-plant.md",
                    "role": "plant-instructions",
                    "visibleGuides": false,
                },
            ],
            "slots": plan.slots,
            "bleedAreas": plan.bleed_areas,
            "trimAreas": plan.trim_areas,
            "cutouts": plan.cutouts,
            "safetyAreas": plan.safety_areas,
            "preflight": preflight,
        });
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap())
    }

    fn golden_preflight_markdown(
        plan: &PlantPrepressPlan,
        preflight: &PlantPrepressPreflightReport,
    ) -> String {
        let mut markdown = format!(
            "# Preflight\n\nTemplate: {}\nOutput condition: {}\nTarget: {} at {} DPI\n\n| Status | Check | Detail |\n| --- | --- | --- |\n",
            plan.template_name,
            plan.output_condition_identifier,
            plan.color_mode.to_uppercase(),
            plan.target_dpi,
        );
        for check in &preflight.checks {
            markdown.push_str(&format!(
                "| {} | {} | {} |\n",
                preflight_status_label(check.status),
                markdown_cell(&check.summary),
                markdown_cell(&check.detail),
            ));
        }
        markdown
    }

    fn golden_pack_readme(plan: &PlantPrepressPlan) -> String {
        format!(
            concat!(
                "# README for Plant\n\n",
                "Production file uses `{}` geometry.\n\n",
                "- Proof file: `proof.svg`, visible guides and calibration marks for human approval only.\n",
                "- Production file: `plant-ready.pdf`, artwork only with guide/template marks removed.\n",
                "- Page: {:.3} x {:.3} mm, {} x {} px at {} DPI.\n",
                "- Color: {}, output condition `{}`.\n",
                "- Slots: {} artwork placement(s).\n\n",
                "Do not print the proof guide layer. Use `plant-ready.pdf` for production.\n"
            ),
            plan.template_name,
            plan.page_width_mm,
            plan.page_height_mm,
            plan.page_width_px,
            plan.page_height_px,
            plan.target_dpi,
            plan.color_mode.to_uppercase(),
            plan.output_condition_identifier,
            plan.slots.len(),
        )
    }

    fn preflight_status_label(status: PlantPrepressPreflightStatus) -> &'static str {
        match status {
            PlantPrepressPreflightStatus::Pass => "pass",
            PlantPrepressPreflightStatus::Warn => "warn",
            PlantPrepressPreflightStatus::Fail => "fail",
        }
    }

    fn markdown_cell(value: &str) -> String {
        value
            .replace('|', "\\|")
            .replace('\n', " ")
            .replace('\r', " ")
    }

    fn cmyk_profile_fingerprint(bytes: &[u8]) -> String {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("fnv1a64-{hash:016x}-len-{}", bytes.len())
    }

    fn assert_png_golden(name: &str, actual: &[u8], mode: GoldenPngMode) {
        let path = prepress_golden_dir().join(name);
        if std::env::var_os("BITNEEDLE_UPDATE_PREPRESS_GOLDENS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
        }
        let expected = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("missing golden {}: {error}", path.display()));
        let expected_pixels = png_pixels(&expected, mode);
        let actual_pixels = png_pixels(actual, mode);
        assert_eq!(
            expected_pixels, actual_pixels,
            "prepress golden mismatch for {}; rerun with BITNEEDLE_UPDATE_PREPRESS_GOLDENS=1 to bless intentional changes",
            path.display(),
        );
    }

    fn assert_text_golden(name: &str, actual: &str) {
        let path = prepress_golden_dir().join(name);
        if std::env::var_os("BITNEEDLE_UPDATE_PREPRESS_GOLDENS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("missing golden {}: {error}", path.display()));
        assert_eq!(
            expected,
            actual,
            "prepress text golden mismatch for {}; rerun with BITNEEDLE_UPDATE_PREPRESS_GOLDENS=1 to bless intentional changes",
            path.display(),
        );
    }

    fn assert_bytes_golden(name: &str, actual: &[u8]) {
        let path = prepress_golden_dir().join(name);
        if std::env::var_os("BITNEEDLE_UPDATE_PREPRESS_GOLDENS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
        }
        let expected = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("missing golden {}: {error}", path.display()));
        assert!(
            expected == actual,
            "prepress binary golden mismatch for {}; expected {} bytes, got {} bytes; rerun with BITNEEDLE_UPDATE_PREPRESS_GOLDENS=1 to bless intentional changes",
            path.display(),
            expected.len(),
            actual.len(),
        );
    }

    fn golden_text_matches_or_update(name: &str, actual: &str) -> bool {
        let path = prepress_golden_dir().join(name);
        if std::env::var_os("BITNEEDLE_UPDATE_PREPRESS_GOLDENS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
        }
        std::fs::read_to_string(&path)
            .map(|expected| expected == actual)
            .unwrap_or(false)
    }

    fn png_pixels(bytes: &[u8], mode: GoldenPngMode) -> (u32, u32, Vec<u8>) {
        let image = image::load_from_memory(bytes).unwrap();
        match mode {
            GoldenPngMode::Rgb => {
                let rgb = image.to_rgb8();
                let (width, height) = rgb.dimensions();
                (width, height, rgb.into_raw())
            }
            GoldenPngMode::Rgba => {
                let rgba = image.to_rgba8();
                let (width, height) = rgba.dimensions();
                (width, height, rgba.into_raw())
            }
        }
    }

    /// Goldens live beside the crate so the tests pass from any checkout, not
    /// only from the monorepo layout this crate was split out of.
    fn prepress_golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("goldenfiles")
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn rgb_png(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};

        let mut out = Vec::new();
        let encoder =
            PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive);
        encoder
            .write_image(rgb, width, height, ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    fn rgba_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};

        let mut out = Vec::new();
        let encoder =
            PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive);
        encoder
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn solid_rgba_png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for chunk in rgba.chunks_exact_mut(4) {
            chunk.copy_from_slice(&pixel);
        }
        rgba_png(&rgba, width, height)
    }

    fn two_up_plan_and_preflight() -> (
        PlantPrepressJob,
        PlantPrepressPlan,
        PlantPrepressPreflightReport,
    ) {
        let job = two_up_job();
        let plan = build_prepress_plan(
            &job,
            &[
                ArtworkInfo {
                    slot_id: "side_a".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
                ArtworkInfo {
                    slot_id: "side_b".to_string(),
                    width_px: 1200,
                    height_px: 1200,
                },
            ],
        )
        .unwrap();
        // The pack builders only read a report; assembling one takes exported
        // PDF bytes, which these tests do not need.
        let preflight = PlantPrepressPreflightReport {
            checks: vec![PlantPrepressPreflightCheck {
                id: "raster-resolution".to_string(),
                status: PlantPrepressPreflightStatus::Pass,
                summary: "Placed artwork meets the template raster minimum".to_string(),
                detail: "Every slot resolves above the plant minimum.".to_string(),
            }],
        };
        (job, plan, preflight)
    }

    #[test]
    fn pack_spec_names_every_pack_member_and_its_plant() {
        let (job, plan, preflight) = two_up_plan_and_preflight();
        let spec: serde_json::Value =
            serde_json::from_str(&pack_spec_json(&job.template, &plan, &preflight).unwrap())
                .unwrap();

        assert_eq!(
            spec["packType"],
            "bitneedle-record-plant-manufacturing-pack"
        );
        assert_eq!(spec["manufacturer"], "Test Plant");
        assert_eq!(spec["product"], "7 in labels");
        assert_eq!(spec["templateId"], plan.template_id);

        let paths: Vec<&str> = spec["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|artifact| artifact["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                PACK_PROOF_PDF_NAME,
                PACK_PLANT_READY_PDF_NAME,
                PACK_SPEC_JSON_NAME,
                PACK_PREFLIGHT_MARKDOWN_NAME,
                PACK_README_NAME,
            ]
        );
        // The production file is the one that goes on press, so it must never
        // be described as carrying guides.
        let production = spec["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["path"] == PACK_PLANT_READY_PDF_NAME)
            .unwrap();
        assert_eq!(production["visibleGuides"], false);
        assert_eq!(spec["slots"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn pack_spec_ends_with_a_newline_for_text_diffs() {
        let (job, plan, preflight) = two_up_plan_and_preflight();
        let spec = pack_spec_json(&job.template, &plan, &preflight).unwrap();
        assert!(spec.ends_with("}\n"));
    }

    #[test]
    fn pack_preflight_markdown_keeps_each_check_on_one_row() {
        let (_, plan, _) = two_up_plan_and_preflight();
        let preflight = PlantPrepressPreflightReport {
            checks: vec![PlantPrepressPreflightCheck {
                id: "pipes".to_string(),
                status: PlantPrepressPreflightStatus::Warn,
                summary: "A | B".to_string(),
                detail: "first\nsecond".to_string(),
            }],
        };
        let markdown = pack_preflight_markdown(&plan, &preflight);
        let row = markdown.lines().last().unwrap();
        assert_eq!(row, "| warn | A \\| B | first second |");
    }

    #[test]
    fn pack_preflight_markdown_falls_back_to_the_check_id() {
        let (_, plan, _) = two_up_plan_and_preflight();
        let preflight = PlantPrepressPreflightReport {
            checks: vec![PlantPrepressPreflightCheck {
                id: "raster-resolution".to_string(),
                status: PlantPrepressPreflightStatus::Pass,
                summary: "   ".to_string(),
                detail: String::new(),
            }],
        };
        assert!(pack_preflight_markdown(&plan, &preflight)
            .contains("| pass | raster-resolution |  |"));
    }

    #[test]
    fn pack_readme_points_the_plant_at_the_production_file() {
        let (_, plan, _) = two_up_plan_and_preflight();
        let readme = pack_readme(&plan);
        assert!(readme.contains("Do not print the proof guide layer."));
        assert!(readme.contains(&format!("Use `{PACK_PLANT_READY_PDF_NAME}` for production.")));
        assert!(readme.contains(&format!("{} DPI", plan.target_dpi)));
        assert!(readme.contains("- Slots: 2 artwork placement(s)."));
    }

    #[test]
    fn pack_readme_trims_trailing_zeros_from_page_dimensions() {
        let (_, mut plan, _) = two_up_plan_and_preflight();
        plan.page_width_mm = 106.0;
        plan.page_height_mm = 98.425;
        assert!(pack_readme(&plan).contains("- Page: 106 x 98.425 mm,"));
    }

    /// Reads the archive the way an unzip tool does: seek the end-of-central
    /// directory, walk its entries, follow each recorded offset to the local
    /// header, and pull the stored bytes back out.
    fn read_stored_zip(archive: &[u8]) -> Vec<(String, Vec<u8>, u32)> {
        let u16_at = |at: usize| u16::from_le_bytes([archive[at], archive[at + 1]]) as usize;
        let u32_at = |at: usize| {
            u32::from_le_bytes([
                archive[at],
                archive[at + 1],
                archive[at + 2],
                archive[at + 3],
            ])
        };

        let eocd = archive.len() - 22;
        assert_eq!(&archive[eocd..eocd + 4], &[0x50, 0x4b, 0x05, 0x06]);
        let entries = u16_at(eocd + 10);
        assert_eq!(entries, u16_at(eocd + 8), "disk and total entry counts agree");
        let mut cursor = u32_at(eocd + 16) as usize;

        let mut members = Vec::with_capacity(entries);
        for _ in 0..entries {
            assert_eq!(&archive[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            let crc = u32_at(cursor + 16);
            let size = u32_at(cursor + 24) as usize;
            let name_len = u16_at(cursor + 28);
            let extra_len = u16_at(cursor + 30);
            let comment_len = u16_at(cursor + 32);
            let local_offset = u32_at(cursor + 42) as usize;
            let name =
                String::from_utf8(archive[cursor + 46..cursor + 46 + name_len].to_vec()).unwrap();

            assert_eq!(
                &archive[local_offset..local_offset + 4],
                &[0x50, 0x4b, 0x03, 0x04],
                "central directory offset for {name} points at a local header"
            );
            assert_eq!(u16_at(local_offset + 8), 0, "{name} is stored, not deflated");
            let data_at =
                local_offset + 30 + u16_at(local_offset + 26) + u16_at(local_offset + 28);
            members.push((name, archive[data_at..data_at + size].to_vec(), crc));
            cursor += 46 + name_len + extra_len + comment_len;
        }
        members
    }

    #[test]
    fn pack_zip_archive_round_trips_every_member() {
        let files = vec![
            (PACK_README_NAME.to_string(), b"# README\n".to_vec()),
            (PACK_SPEC_JSON_NAME.to_string(), b"{}\n".to_vec()),
            (PACK_PLANT_READY_PDF_NAME.to_string(), vec![0u8; 4096]),
        ];
        let archive = pack_zip_archive(&files);
        assert_eq!(&archive[0..4], &[0x50, 0x4b, 0x03, 0x04]);

        let members = read_stored_zip(&archive);
        assert_eq!(members.len(), files.len());
        for ((name, bytes), (read_name, read_bytes, crc)) in files.iter().zip(members) {
            assert_eq!(&read_name, name);
            assert_eq!(&read_bytes, bytes);
            assert_eq!(crc, zip_crc32(bytes), "{name} records its own CRC");
        }
    }

    #[test]
    fn pack_zip_archive_of_nothing_is_still_a_readable_archive() {
        let archive = pack_zip_archive(&[]);
        assert!(read_stored_zip(&archive).is_empty());
    }

    #[test]
    fn a_proof_pdf_opens_at_the_label_size_and_carries_its_raster() {
        let jpeg = b"\xff\xd8\xff\xe0 pretend jpeg \xff\xd9";
        let pdf = proof_pdf_from_jpeg(jpeg, 98.4, 98.4, 1163, 1163, "Proof").unwrap();

        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        // 98.4 mm is 278.929 pt; a reader that opens this at any other size
        // would print the label at the wrong physical diameter.
        assert!(text.contains("/MediaBox [0 0 278.929 278.929]"), "{text:.400}");
        assert!(text.contains("/Filter /DCTDecode"));
        assert!(text.contains("/Width 1163"));
        assert!(text.contains("startxref"));
        assert!(pdf
            .windows(jpeg.len())
            .any(|window| window == jpeg.as_slice()));
    }

    #[test]
    fn a_proof_pdf_refuses_input_it_cannot_place() {
        assert!(proof_pdf_from_jpeg(b"", 98.4, 98.4, 10, 10, "Proof").is_err());
        assert!(proof_pdf_from_jpeg(b"jpeg", 98.4, 98.4, 0, 10, "Proof").is_err());
        assert!(proof_pdf_from_jpeg(b"jpeg", 0.0, 98.4, 10, 10, "Proof").is_err());
        assert!(proof_pdf_from_jpeg(b"jpeg", f64::NAN, 98.4, 10, 10, "Proof").is_err());
    }

    #[test]
    fn a_proof_pdf_title_cannot_break_out_of_its_string() {
        let pdf = proof_pdf_from_jpeg(b"jpeg", 98.4, 98.4, 10, 10, "A (weird)\\ title\nline").unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Title (A \\(weird\\)\\\\ title line)"), "{text:.600}");
    }

    #[test]
    fn pack_zip_crc_matches_the_published_check_value() {
        // The CRC-32 of "123456789" is the standard check value for this
        // polynomial; a wrong table or bit order fails here rather than in a
        // plant's unzip.
        assert_eq!(zip_crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(zip_crc32(b""), 0);
    }
}
