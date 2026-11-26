use alloc::string::{String, ToString};
use serde_json::{Value, json};

use crate::{PluginError, invalid, run_cmd};

pub fn v_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn v_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn v_scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn v_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn v_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn v_len(a: [f64; 3]) -> f64 {
    libm::sqrt(v_dot(a, a))
}

pub fn v_norm(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = v_len(a);
    if l < 1e-9 {
        None
    } else {
        Some([a[0] / l, a[1] / l, a[2] / l])
    }
}

pub fn parse_vec3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

#[derive(Clone)]
pub struct Placement {
    pub origin: [f64; 3],
    pub x_dir: [f64; 3],
    pub y_dir: [f64; 3],
    pub z_dir: [f64; 3],
    pub target_body: Option<String>,
}

impl Placement {
    pub fn from_origin_normal(
        origin: [f64; 3],
        normal: [f64; 3],
        target_body: Option<String>,
    ) -> Self {
        let z = v_norm(normal).unwrap_or([0.0, 0.0, 1.0]);
        let ref_v = if libm::fabs(z[2]) < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let x = v_norm(v_cross(ref_v, z)).unwrap_or([1.0, 0.0, 0.0]);
        let y = v_norm(v_cross(z, x)).unwrap_or([0.0, 1.0, 0.0]);
        Self {
            origin,
            x_dir: x,
            y_dir: y,
            z_dir: z,
            target_body,
        }
    }

    pub fn xy_default() -> Self {
        Self {
            origin: [0.0, 0.0, 0.0],
            x_dir: [1.0, 0.0, 0.0],
            y_dir: [0.0, 1.0, 0.0],
            z_dir: [0.0, 0.0, 1.0],
            target_body: None,
        }
    }

    pub fn flip(mut self) -> Self {
        self.z_dir = v_scale(self.z_dir, -1.0);
        self.y_dir = v_scale(self.y_dir, -1.0);
        self
    }

    /// Radial-axial sketch plane: sketch X = +r (along x_dir), sketch Y = +z (along z_dir).
    pub fn radial_plane_json(&self, opposite_start: bool) -> Value {
        let rx = if opposite_start {
            v_scale(self.x_dir, -1.0)
        } else {
            self.x_dir
        };
        json!({
            "origin": [self.origin[0], self.origin[1], self.origin[2]],
            "x_dir": [rx[0], rx[1], rx[2]],
            "y_dir": [self.z_dir[0], self.z_dir[1], self.z_dir[2]],
        })
    }
}

fn circumcenter_3d(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let ab = v_sub(b, a);
    let ac = v_sub(c, a);
    let n = v_cross(ab, ac);
    let n2 = v_dot(n, n);
    if n2 < 1e-10 {
        return None;
    }
    let ab2 = v_dot(ab, ab);
    let ac2 = v_dot(ac, ac);
    let term1 = v_scale(v_cross(n, ab), ac2);
    let term2 = v_scale(v_cross(ac, n), ab2);
    let offset = v_scale(v_add(term1, term2), 1.0 / (2.0 * n2));
    Some((v_add(a, offset), v_norm(n)?))
}

fn resolve_plane_pick(pick: &Value) -> Result<Placement, PluginError> {
    let ptype = pick.get("type").and_then(Value::as_str).unwrap_or("");
    match ptype {
        "face" => {
            let body = pick
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("plane", "selected face is missing its owning body"))?;
            let faces_res = run_cmd("model.faces", json!({ "body": body }))?;
            let faces = faces_res
                .get("faces")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("plane", "selected body has no faces"))?;
            let target_idx = pick.get("index").and_then(Value::as_u64);
            let face = target_idx
                .and_then(|idx| {
                    faces
                        .iter()
                        .find(|f| f.get("index").and_then(Value::as_u64) == Some(idx))
                })
                .or_else(|| {
                    pick.get("point").and_then(parse_vec3).and_then(|pt| {
                        faces.iter().min_by(|a, b| {
                            let da = a
                                .get("centroid")
                                .and_then(parse_vec3)
                                .map(|c| v_len(v_sub(c, pt)))
                                .unwrap_or(1e9);
                            let db = b
                                .get("centroid")
                                .and_then(parse_vec3)
                                .map(|c| v_len(v_sub(c, pt)))
                                .unwrap_or(1e9);
                            da.total_cmp(&db)
                        })
                    })
                })
                .ok_or_else(|| invalid("plane", "picked face could not be resolved"))?;
            let normal = face.get("plane_normal").and_then(parse_vec3).ok_or_else(|| {
                invalid(
                    "plane",
                    "selected face is not planar; please pick a flat face or construction plane",
                )
            })?;
            let origin = face
                .get("centroid")
                .and_then(parse_vec3)
                .or_else(|| pick.get("point").and_then(parse_vec3))
                .unwrap_or([0.0, 0.0, 0.0]);
            Ok(Placement::from_origin_normal(
                origin,
                normal,
                Some(body.to_string()),
            ))
        }
        "plane" => {
            let name = pick.get("name").and_then(Value::as_str).unwrap_or("XY");
            match name {
                "XY" => Ok(Placement::xy_default()),
                "XZ" => Ok(Placement::from_origin_normal(
                    [0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    None,
                )),
                "YZ" => Ok(Placement::from_origin_normal(
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    None,
                )),
                other => {
                    let res = run_cmd(
                        "inspect.measure",
                        json!({ "items": [{ "type": "plane", "name": other }] }),
                    )?;
                    let item = res
                        .get("items")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .ok_or_else(|| invalid("plane", "construction plane could not be measured"))?;
                    let origin = item
                        .get("origin")
                        .and_then(parse_vec3)
                        .unwrap_or([0.0, 0.0, 0.0]);
                    let normal = item
                        .get("normal")
                        .and_then(parse_vec3)
                        .unwrap_or([0.0, 0.0, 1.0]);
                    Ok(Placement::from_origin_normal(origin, normal, None))
                }
            }
        }
        _ => Err(invalid(
            "plane",
            "Plane must be a planar face or construction plane",
        )),
    }
}

fn resolve_point_pick(pick: &Value) -> Result<([f64; 3], Option<[f64; 3]>, Option<String>), PluginError> {
    let ptype = pick.get("type").and_then(Value::as_str).unwrap_or("");
    match ptype {
        "edge" => {
            let body = pick
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("point", "selected edge is missing its owning body"))?;
            let edges_res = run_cmd("model.edges", json!({ "body": body }))?;
            let edges = edges_res
                .get("edges")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("point", "selected body has no edges"))?;
            let target_idx = pick.get("index").and_then(Value::as_u64);
            let edge = target_idx
                .and_then(|idx| {
                    edges
                        .iter()
                        .find(|e| e.get("index").and_then(Value::as_u64) == Some(idx))
                })
                .or_else(|| edges.first())
                .ok_or_else(|| invalid("point", "picked edge could not be resolved"))?;
            let s = edge.get("start").and_then(parse_vec3).unwrap_or([0.0, 0.0, 0.0]);
            let m = edge.get("mid").and_then(parse_vec3).unwrap_or(s);
            let e = edge.get("end").and_then(parse_vec3).unwrap_or(s);

            if v_len(v_sub(s, e)) < 1e-4 && v_len(v_sub(s, m)) > 1e-4 {
                // Full circular edge: start and mid are opposite points on the circle.
                let center = v_scale(v_add(s, m), 0.5);
                let mut normal_hint = None;
                if let Ok(faces_res) = run_cmd("model.faces", json!({ "body": body })) {
                    if let Some(faces) = faces_res.get("faces").and_then(Value::as_array) {
                        let radial = v_norm(v_sub(s, center)).unwrap_or([1.0, 0.0, 0.0]);
                        for f in faces {
                            if let (Some(n), Some(fc)) = (
                                f.get("plane_normal").and_then(parse_vec3),
                                f.get("centroid").and_then(parse_vec3),
                            ) {
                                if libm::fabs(v_dot(v_sub(center, fc), n)) < 1e-2
                                    && libm::fabs(v_dot(radial, n)) < 1e-2
                                {
                                    normal_hint = Some(n);
                                    break;
                                }
                            }
                        }
                    }
                }
                Ok((center, normal_hint, Some(body.to_string())))
            } else if let Some((center, norm)) = circumcenter_3d(s, m, e) {
                Ok((center, Some(norm), Some(body.to_string())))
            } else {
                Ok((m, None, Some(body.to_string())))
            }
        }
        "sketch_point" => {
            if let Some(pt) = pick.get("point").and_then(parse_vec3) {
                return Ok((pt, None, None));
            }
            if let Some(sk) = pick.get("sketch") {
                if let Ok(sk_res) = run_cmd("sketch.inspect", json!({ "sketch": sk })) {
                    if let Some(plane) = sk_res.get("plane").and_then(Value::as_object) {
                        let o = plane
                            .get("origin")
                            .and_then(parse_vec3)
                            .unwrap_or([0.0, 0.0, 0.0]);
                        return Ok((o, None, None));
                    }
                }
            }
            Err(invalid("point", "could not determine coordinates of picked sketch point"))
        }
        _ => {
            if let Some(pt) = pick.get("point").and_then(parse_vec3) {
                Ok((pt, None, None))
            } else {
                Err(invalid(
                    "point",
                    "Point must be a circular edge or sketch point",
                ))
            }
        }
    }
}

pub fn resolve_luer_placement(
    point_picks: &[Value],
    plane_picks: &[Value],
    flip: bool,
) -> Result<Placement, PluginError> {
    let mut placement = if let Some(plane_pick) = plane_picks.first() {
        resolve_plane_pick(plane_pick)?
    } else {
        Placement::xy_default()
    };

    if let Some(point_pick) = point_picks.first() {
        let (pt, normal_hint, body_hint) = resolve_point_pick(point_pick)?;
        if plane_picks.is_empty() {
            if let Some(n) = normal_hint {
                placement = Placement::from_origin_normal(pt, n, body_hint);
            } else {
                placement.origin = pt;
                placement.target_body = body_hint;
            }
        } else {
            // Project locating point orthogonally onto the chosen plane so the fitting sits flush.
            let delta = v_sub(pt, placement.origin);
            let along_n = v_dot(delta, placement.z_dir);
            placement.origin = v_sub(pt, v_scale(placement.z_dir, along_n));
        }
    }

    if flip {
        placement = placement.flip();
    }
    Ok(placement)
}
