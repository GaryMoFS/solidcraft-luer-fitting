use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use serde_json::{Value, json};

use crate::placement::Placement;
use crate::{PluginError, invalid, list_existing_doc_names, report_progress, run_cmd, unique_name};

// ISO 80369-7 (Sub-standard 7) nominal Luer fitting constants (mm)
const TAPER_SLOPE_RADIAL: f64 = 0.03; // 6% diametral taper -> 0.03 mm radius per 1 mm length
const CONE_LENGTH: f64 = 7.50;

// Male 6% cone
const MALE_TIP_OD_NOM: f64 = 3.97; // ISO 80369-7 Table B.1 nominal tip diameter
const MALE_TIP_CHAMFER: f64 = 0.20;
const MIN_CONE_WALL: f64 = 0.25;

// Male Lock collar (internal double-start RH trapezoidal thread)
const MALE_COLLAR_FLOOR: f64 = 1.00; // base floor so standalone Male Lock is 1 solid body
const MALE_COLLAR_RIM_ABOVE_FLOOR: f64 = 5.40; // cone protrudes 2.10 mm past collar rim
const MALE_COLLAR_OUTER_R: f64 = 5.00; // 10.0 mm collar OD
const MALE_COLLAR_MINOR_D_NOM: f64 = 7.00; // thread crest ID
const MALE_COLLAR_MAJOR_D_NOM: f64 = 8.00; // thread root ID (0.50 mm thread depth)
const MALE_COLLAR_CHAMFER: f64 = 0.25;

// Female 6% socket & hub
const FEMALE_MOUTH_ID_NOM: f64 = 4.29; // ISO 80369-7 Table B.1 nominal mouth ID
const FEMALE_BOSS_OD_NOM: f64 = 6.73; // standard female Luer hub OD
const FEMALE_ENTRY_CHAMFER: f64 = 0.20;
const FEMALE_OUTER_CHAMFER: f64 = 0.25;

// Female Lock external double-start RH trapezoidal thread
const FEMALE_THREAD_MAJOR_D_NOM: f64 = 7.83;

// Double-start ISO 80369-7 trapezoidal thread parameters
const THREAD_LEAD: f64 = 5.00; // 2 starts * 2.50 mm pitch
const THREAD_FLANK_TAN: f64 = 0.466_307_658_154_998_6; // tan(25 deg)
const FEMALE_CREST_WIDTH: f64 = 0.55;
const MALE_GROOVE_ROOT_WIDTH: f64 = 0.65;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FittingType {
    MaleSlip,
    MaleLock,
    MaleLockInternal,
    FemaleSlip,
    FemaleSlipInternal,
    FemaleLock,
}

impl FittingType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "male_slip" => Some(Self::MaleSlip),
            "male_lock" => Some(Self::MaleLock),
            "male_lock_internal" => Some(Self::MaleLockInternal),
            "female_slip" => Some(Self::FemaleSlip),
            "female_slip_internal" => Some(Self::FemaleSlipInternal),
            "female_lock" => Some(Self::FemaleLock),
            _ => None,
        }
    }

    pub fn is_male(self) -> bool {
        matches!(self, Self::MaleSlip | Self::MaleLock | Self::MaleLockInternal)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MaleSlip => "Luer_Male_Slip",
            Self::MaleLock => "Luer_Male_Lock",
            Self::MaleLockInternal => "Luer_Male_Lock_Internal",
            Self::FemaleSlip => "Luer_Female_Slip",
            Self::FemaleSlipInternal => "Luer_Female_Slip_Internal",
            Self::FemaleLock => "Luer_Female_Lock",
        }
    }
}

pub fn round3(v: f64) -> f64 {
    libm::round(v * 1000.0) / 1000.0
}

pub fn max_safe_hole_diameter(clearance: f64) -> f64 {
    let tip_od = MALE_TIP_OD_NOM - clearance;
    let max_d = tip_od - 2.0 * MIN_CONE_WALL;
    if max_d < 0.2 { 0.2 } else { max_d }
}

fn draw_closed_polygon(
    plane_json: &Value,
    sketch_name: &str,
    pts: &[(f64, f64)],
) -> Result<String, PluginError> {
    let sk_res = run_cmd(
        "sketch.create",
        json!({
            "plane": plane_json,
            "name": sketch_name
        }),
    )?;
    let sk_id = sk_res
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(sketch_name)
        .to_string();

    let mut poly = alloc::vec::Vec::with_capacity(pts.len() + 1);
    for &(x, y) in pts {
        poly.push(json!([x, y]));
    }
    if let Some(&(x0, y0)) = pts.first() {
        poly.push(json!([x0, y0]));
    }
    run_cmd("sketch.line", json!({ "points": poly }))?;
    run_cmd("sketch.finish", json!({}))?;
    Ok(sk_id)
}

fn revolve_sketch(
    sketch_name: &str,
    operation: &str,
    targets: Option<&str>,
    body_name: Option<&str>,
    feature_name: &str,
) -> Result<String, PluginError> {
    let mut args = serde_json::Map::new();
    args.insert("sketch".to_string(), json!(sketch_name));
    args.insert("axis".to_string(), json!("y"));
    args.insert("angle".to_string(), json!("360 deg"));
    args.insert("operation".to_string(), json!(operation));
    args.insert("name".to_string(), json!(feature_name));
    if let Some(t) = targets {
        args.insert("targets".to_string(), json!([t]));
    }
    if let Some(b) = body_name {
        args.insert("body_name".to_string(), json!(b));
    }
    let res = run_cmd("solid.revolve", Value::Object(args))?;
    Ok(revolved_body_name(&res, targets, body_name))
}

/// The body a revolve produced or changed.
///
/// `solid.revolve` replies with `bodies` (every body in the design), plus `created_bodies` and
/// `changed_bodies` for this feature alone. Never read the first entry of `bodies`: in a design
/// that already has a body it names that unrelated body, and the thread and pocket steps that
/// follow would then cut or join the wrong part.
fn revolved_body_name(res: &Value, targets: Option<&str>, body_name: Option<&str>) -> String {
    let names = |key: &str| -> alloc::vec::Vec<&str> {
        res.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    };
    if let Some(target) = targets {
        // Join or cut: the target keeps its name; fall back to what the feature says it changed.
        let changed = names("changed_bodies");
        if changed.is_empty() || changed.contains(&target) {
            return target.to_string();
        }
        return changed[0].to_string();
    }
    // New body: the feature's own created body, then the name we asked for.
    names("created_bodies")
        .first()
        .copied()
        .or_else(|| res.get("body").and_then(Value::as_str))
        .or(body_name)
        .unwrap_or("Body")
        .to_string()
}

fn apply_double_start_thread(
    placement: &Placement,
    profile_pts: &[(f64, f64)],
    height_mm: f64,
    operation: &str,
    target_body: &str,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<(), PluginError> {
    for (idx, opposite) in [(1, false), (2, true)] {
        let sk_name = unique_name(&format!("{base_name}_ThreadSk{idx}"), existing);
        let plane = placement.radial_plane_json(opposite);
        let sk_id = draw_closed_polygon(&plane, &sk_name, profile_pts)?;

        let feat_name = unique_name(&format!("{base_name}_Thread{idx}"), existing);
        run_cmd(
            "solid.coil",
            json!({
                "sketch": sk_id,
                "base": [placement.origin[0], placement.origin[1], placement.origin[2]],
                "axis": [placement.z_dir[0], placement.z_dir[1], placement.z_dir[2]],
                "pitch": THREAD_LEAD,
                "height": height_mm,
                "clockwise": false,
                "operation": operation,
                "targets": [target_body],
                "name": feat_name
            }),
        )?;
    }
    Ok(())
}

pub fn run_luer_fitting(raw: &Value) -> Result<String, PluginError> {
    report_progress(0.05, "Validating Luer fitting parameters")?;

    let type_str = raw
        .get("fitting_type")
        .and_then(Value::as_str)
        .unwrap_or("male_lock");
    let fitting_type = FittingType::parse(type_str)
        .ok_or_else(|| invalid("fitting_type", format!("unsupported fitting type `{type_str}`")))?;

    let clearance = raw
        .get("clearance")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if !clearance.is_finite() || clearance < -0.5 || clearance > 0.5 {
        return Err(invalid(
            "clearance",
            "Clearance must be between -0.50 mm and +0.50 mm",
        ));
    }

    let hole_diameter = raw
        .get("hole_diameter")
        .and_then(Value::as_f64)
        .unwrap_or(2.25);
    if fitting_type.is_male() {
        let max_hole = max_safe_hole_diameter(clearance);
        if !hole_diameter.is_finite() || hole_diameter < 0.2 {
            return Err(invalid(
                "hole_diameter",
                "Hole diameter must be at least 0.20 mm",
            ));
        }
        if hole_diameter > max_hole {
            return Err(invalid(
                "hole_diameter",
                format!(
                    "Hole diameter ({:.2} mm) is too large for the male Luer cone tip (max {:.2} mm at {:.2} mm clearance)",
                    hole_diameter, max_hole, clearance
                ),
            ));
        }
    }

    let flip = raw
        .get("flip_direction")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let point_picks = raw
        .get("point")
        .and_then(Value::as_array)
        .map(alloc::vec::Vec::as_slice)
        .unwrap_or(&[]);
    let plane_picks = raw
        .get("plane")
        .and_then(Value::as_array)
        .map(alloc::vec::Vec::as_slice)
        .unwrap_or(&[]);

    report_progress(0.15, "Resolving placement plane and point")?;
    let placement = crate::placement::resolve_luer_placement(point_picks, plane_picks, flip)?;

    let mut existing = list_existing_doc_names()?;
    let base_name = unique_name(fitting_type.label(), &mut existing);
    let dr = 0.5 * clearance;

    let result_body = match fitting_type {
        FittingType::MaleSlip => build_male_slip(&placement, hole_diameter, dr, &base_name, &mut existing)?,
        FittingType::MaleLock => build_male_lock(&placement, hole_diameter, dr, &base_name, &mut existing)?,
        FittingType::MaleLockInternal => {
            build_male_lock_internal(&placement, hole_diameter, dr, &base_name, &mut existing)?
        }
        FittingType::FemaleSlip => build_female_slip(&placement, dr, &base_name, &mut existing)?,
        FittingType::FemaleSlipInternal => {
            build_female_slip_internal(&placement, dr, &base_name, &mut existing)?
        }
        FittingType::FemaleLock => build_female_lock(&placement, dr, &base_name, &mut existing)?,
    };

    report_progress(1.0, "Luer fitting created")?;
    Ok(json!({
        "standard": "ISO 80369-7",
        "fitting_type": type_str,
        "body": result_body,
        "clearance_mm": clearance,
        "hole_diameter_mm": if fitting_type.is_male() { json!(hole_diameter) } else { Value::Null },
        "joined_to_existing_body": placement.target_body.is_some()
    })
    .to_string())
}

fn build_male_slip(
    placement: &Placement,
    hole_diameter: f64,
    dr: f64,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<String, PluginError> {
    report_progress(0.4, "Building Male Slip 6% taper cone")?;
    let r_hole = 0.5 * hole_diameter;
    let r_tip = 0.5 * MALE_TIP_OD_NOM - dr;
    let r_base = r_tip + TAPER_SLOPE_RADIAL * CONE_LENGTH;
    let ch = MALE_TIP_CHAMFER;

    let pts = [
        (r_hole, 0.0),
        (r_base, 0.0),
        (r_tip + TAPER_SLOPE_RADIAL * ch, CONE_LENGTH - ch),
        (r_tip - ch, CONE_LENGTH),
        (r_hole, CONE_LENGTH),
    ];

    let sk_name = unique_name(&format!("{base_name}_Sk"), existing);
    let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &sk_name, &pts)?;
    let (op, target, bname) = match placement.target_body.as_deref() {
        Some(t) => ("join", Some(t), None),
        None => ("new_body", None, Some(base_name)),
    };
    let feat_name = unique_name(&format!("{base_name}_Rev"), existing);
    revolve_sketch(&sk_id, op, target, bname, &feat_name)
}

fn build_male_lock(
    placement: &Placement,
    hole_diameter: f64,
    dr: f64,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<String, PluginError> {
    report_progress(0.35, "Building Male Lock cone and collar")?;
    let r_hole = 0.5 * hole_diameter;
    let z_floor = MALE_COLLAR_FLOOR;
    let z_rim = z_floor + MALE_COLLAR_RIM_ABOVE_FLOOR;
    let z_tip = z_floor + CONE_LENGTH;

    let r_tip = 0.5 * MALE_TIP_OD_NOM - dr;
    let r_base = r_tip + TAPER_SLOPE_RADIAL * CONE_LENGTH;
    let r_collar_in = 0.5 * MALE_COLLAR_MINOR_D_NOM + dr;
    let r_collar_out = MALE_COLLAR_OUTER_R;
    let ch_tip = MALE_TIP_CHAMFER;
    let ch_rim = MALE_COLLAR_CHAMFER;

    // Single connected revolve: base floor + outer collar + inner 6% tapered cone + through bore
    let pts = [
        (r_hole, 0.0),
        (r_collar_out, 0.0),
        (r_collar_out, z_rim - ch_rim),
        (r_collar_out - ch_rim, z_rim),
        (r_collar_in + ch_rim, z_rim),
        (r_collar_in, z_rim - ch_rim),
        (r_collar_in, z_floor),
        (r_base, z_floor),
        (r_tip + TAPER_SLOPE_RADIAL * ch_tip, z_tip - ch_tip),
        (r_tip - ch_tip, z_tip),
        (r_hole, z_tip),
    ];

    let sk_name = unique_name(&format!("{base_name}_Sk"), existing);
    let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &sk_name, &pts)?;
    let (op, target, bname) = match placement.target_body.as_deref() {
        Some(t) => ("join", Some(t), None),
        None => ("new_body", None, Some(base_name)),
    };
    let feat_name = unique_name(&format!("{base_name}_Rev"), existing);
    let body = revolve_sketch(&sk_id, op, target, bname, &feat_name)?;

    report_progress(0.65, "Cutting ISO 80369-7 double-start lock thread")?;
    // Trapezoidal internal thread groove profile, swept upward from z_start past z_rim
    let r_root = 0.5 * MALE_COLLAR_MAJOR_D_NOM + dr;
    let r_inner_clear = r_collar_in - 0.25; // extends into air inside collar, clear of cone
    let half_root = 0.5 * MALE_GROOVE_ROOT_WIDTH;
    let half_inner = half_root + (r_root - r_inner_clear) * THREAD_FLANK_TAN;
    let z_start = z_floor + 0.65;
    let sweep_h = (z_rim - z_start) + 0.60;

    let thread_pts = [
        (r_inner_clear, z_start - half_inner),
        (r_root, z_start - half_root),
        (r_root, z_start + half_root),
        (r_inner_clear, z_start + half_inner),
    ];
    apply_double_start_thread(
        placement,
        &thread_pts,
        sweep_h,
        "cut",
        &body,
        base_name,
        existing,
    )?;

    Ok(body)
}

fn build_male_lock_internal(
    placement: &Placement,
    hole_diameter: f64,
    dr: f64,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<String, PluginError> {
    report_progress(0.3, "Carving recessed Male Lock pocket")?;
    let r_hole = 0.5 * hole_diameter;
    let r_tip = 0.5 * MALE_TIP_OD_NOM - dr;
    let r_base = r_tip + TAPER_SLOPE_RADIAL * CONE_LENGTH;
    let r_collar_in = 0.5 * MALE_COLLAR_MINOR_D_NOM + dr;
    let ch_tip = MALE_TIP_CHAMFER;
    let ch_rim = MALE_COLLAR_CHAMFER;
    let z_floor = -CONE_LENGTH;

    // If placed on a construction plane with no target body, create a standalone block first
    let target_body = if let Some(t) = placement.target_body.as_deref() {
        t.to_string()
    } else {
        let blk_sk = unique_name(&format!("{base_name}_HostSk"), existing);
        let blk_pts = [
            (0.0, z_floor - MALE_COLLAR_FLOOR),
            (MALE_COLLAR_OUTER_R, z_floor - MALE_COLLAR_FLOOR),
            (MALE_COLLAR_OUTER_R, 0.0),
            (0.0, 0.0),
        ];
        let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &blk_sk, &blk_pts)?;
        let feat = unique_name(&format!("{base_name}_Host"), existing);
        revolve_sketch(&sk_id, "new_body", None, Some(base_name), &feat)?
    };

    // 1. Cut annular lock cavity around the flush 6% cone (z_floor .. +0.05)
    let cav_pts = [
        (r_base, z_floor),
        (r_collar_in, z_floor),
        (r_collar_in, -ch_rim),
        (r_collar_in + ch_rim, 0.05),
        (r_tip - ch_tip, 0.05),
        (r_tip - ch_tip, 0.0),
        (r_tip + TAPER_SLOPE_RADIAL * ch_tip, -ch_tip),
    ];
    let cav_sk = unique_name(&format!("{base_name}_CavSk"), existing);
    let cav_sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &cav_sk, &cav_pts)?;
    let cav_feat = unique_name(&format!("{base_name}_CavCut"), existing);
    revolve_sketch(&cav_sk_id, "cut", Some(&target_body), None, &cav_feat)?;

    // 2. Cut central bore through the recessed cone (z_floor .. +0.05)
    let bore_pts = [
        (0.0, z_floor),
        (r_hole, z_floor),
        (r_hole, 0.05),
        (0.0, 0.05),
    ];
    let bore_sk = unique_name(&format!("{base_name}_BoreSk"), existing);
    let bore_sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &bore_sk, &bore_pts)?;
    let bore_feat = unique_name(&format!("{base_name}_BoreCut"), existing);
    revolve_sketch(&bore_sk_id, "cut", Some(&target_body), None, &bore_feat)?;

    report_progress(0.65, "Cutting internal double-start lock thread")?;
    // 3. Cut double-start internal trapezoidal thread grooves on pocket wall
    let r_root = 0.5 * MALE_COLLAR_MAJOR_D_NOM + dr;
    let r_inner_clear = r_collar_in - 0.25;
    let half_root = 0.5 * MALE_GROOVE_ROOT_WIDTH;
    let half_inner = half_root + (r_root - r_inner_clear) * THREAD_FLANK_TAN;
    let z_start = z_floor + 1.00;
    let sweep_h = -z_start + 0.60;

    let thread_pts = [
        (r_inner_clear, z_start - half_inner),
        (r_root, z_start - half_root),
        (r_root, z_start + half_root),
        (r_inner_clear, z_start + half_inner),
    ];
    apply_double_start_thread(
        placement,
        &thread_pts,
        sweep_h,
        "cut",
        &target_body,
        base_name,
        existing,
    )?;

    Ok(target_body)
}

fn build_female_slip(
    placement: &Placement,
    dr: f64,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<String, PluginError> {
    report_progress(0.4, "Building Female Slip 6% taper boss")?;
    let r_mouth = 0.5 * FEMALE_MOUTH_ID_NOM + dr;
    let r_root = r_mouth - TAPER_SLOPE_RADIAL * CONE_LENGTH;
    let r_out = 0.5 * FEMALE_BOSS_OD_NOM;
    let ch_in = FEMALE_ENTRY_CHAMFER;
    let ch_out = FEMALE_OUTER_CHAMFER;

    let pts = [
        (r_root, 0.0),
        (r_out, 0.0),
        (r_out, CONE_LENGTH - ch_out),
        (r_out - ch_out, CONE_LENGTH),
        (r_mouth + ch_in, CONE_LENGTH),
        (r_mouth - TAPER_SLOPE_RADIAL * ch_in, CONE_LENGTH - ch_in),
    ];

    let sk_name = unique_name(&format!("{base_name}_Sk"), existing);
    let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &sk_name, &pts)?;
    let (op, target, bname) = match placement.target_body.as_deref() {
        Some(t) => ("join", Some(t), None),
        None => ("new_body", None, Some(base_name)),
    };
    let feat_name = unique_name(&format!("{base_name}_Rev"), existing);
    revolve_sketch(&sk_id, op, target, bname, &feat_name)
}

fn build_female_slip_internal(
    placement: &Placement,
    dr: f64,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<String, PluginError> {
    report_progress(0.4, "Cutting recessed Female Slip 6% taper socket")?;
    let r_mouth = 0.5 * FEMALE_MOUTH_ID_NOM + dr;
    let r_root = r_mouth - TAPER_SLOPE_RADIAL * CONE_LENGTH;
    let ch_in = FEMALE_ENTRY_CHAMFER;

    let target_body = if let Some(t) = placement.target_body.as_deref() {
        t.to_string()
    } else {
        let blk_sk = unique_name(&format!("{base_name}_HostSk"), existing);
        let r_out = 0.5 * FEMALE_BOSS_OD_NOM;
        let blk_pts = [
            (0.0, -CONE_LENGTH - 1.0),
            (r_out, -CONE_LENGTH - 1.0),
            (r_out, 0.0),
            (0.0, 0.0),
        ];
        let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &blk_sk, &blk_pts)?;
        let feat = unique_name(&format!("{base_name}_Host"), existing);
        revolve_sketch(&sk_id, "new_body", None, Some(base_name), &feat)?
    };

    let cut_pts = [
        (0.0, -CONE_LENGTH),
        (r_root, -CONE_LENGTH),
        (r_mouth - TAPER_SLOPE_RADIAL * ch_in, -ch_in),
        (r_mouth + ch_in, 0.05),
        (0.0, 0.05),
    ];
    let sk_name = unique_name(&format!("{base_name}_CutSk"), existing);
    let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &sk_name, &cut_pts)?;
    let feat_name = unique_name(&format!("{base_name}_Cut"), existing);
    revolve_sketch(&sk_id, "cut", Some(&target_body), None, &feat_name)?;
    Ok(target_body)
}

fn build_female_lock(
    placement: &Placement,
    dr: f64,
    base_name: &str,
    existing: &mut BTreeSet<String>,
) -> Result<String, PluginError> {
    report_progress(0.35, "Building Female Lock 6% taper hub")?;
    let r_mouth = 0.5 * FEMALE_MOUTH_ID_NOM + dr;
    let r_root = r_mouth - TAPER_SLOPE_RADIAL * CONE_LENGTH;
    let r_core = 0.5 * FEMALE_BOSS_OD_NOM - dr;
    let ch_in = FEMALE_ENTRY_CHAMFER;
    let ch_out = FEMALE_OUTER_CHAMFER;

    let pts = [
        (r_root, 0.0),
        (r_core, 0.0),
        (r_core, CONE_LENGTH - ch_out),
        (r_core - ch_out, CONE_LENGTH),
        (r_mouth + ch_in, CONE_LENGTH),
        (r_mouth - TAPER_SLOPE_RADIAL * ch_in, CONE_LENGTH - ch_in),
    ];

    let sk_name = unique_name(&format!("{base_name}_Sk"), existing);
    let sk_id = draw_closed_polygon(&placement.radial_plane_json(false), &sk_name, &pts)?;
    let (op, target, bname) = match placement.target_body.as_deref() {
        Some(t) => ("join", Some(t), None),
        None => ("new_body", None, Some(base_name)),
    };
    let feat_name = unique_name(&format!("{base_name}_Rev"), existing);
    let body = revolve_sketch(&sk_id, op, target, bname, &feat_name)?;

    report_progress(0.65, "Building external double-start lock thread")?;
    let r_maj = 0.5 * FEMALE_THREAD_MAJOR_D_NOM - dr;
    let r_embed = r_core - 0.20; // embedded cleanly inside hub wall
    let half_crest = 0.5 * FEMALE_CREST_WIDTH;
    let half_root = half_crest + (r_maj - r_embed) * THREAD_FLANK_TAN;
    let z_start = 0.90;
    let sweep_h = 5.40; // stays cleanly within [0.2, 6.9] inside [0, 7.50]

    let thread_pts = [
        (r_embed, z_start - half_root),
        (r_maj, z_start - half_crest),
        (r_maj, z_start + half_crest),
        (r_embed, z_start + half_root),
    ];
    apply_double_start_thread(
        placement,
        &thread_pts,
        sweep_h,
        "join",
        &body,
        base_name,
        existing,
    )?;

    Ok(body)
}
