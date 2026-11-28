//! Bidirectional PAULGA Send Mode mesh-bundle exchange (`export-selected-mesh`, `import-mesh-bundle`).
//!
//! Shares the exact `schemaVersion: "3.1"` JSON bundle contract used by AutoCAD, ZWCAD, GstarCAD,
//! Rhino, Revit, SketchUp and Blender so PAULGA CAD exchanges geometry directly with every host.

use std::collections::HashSet;
use std::f64::consts::TAU;
use std::path::Path;

use cadcraft_doc::{Entity, EntityKind, Handle};
use serde_json::{Map, Value, json};

use crate::CadApp;

const SCHEMA_VERSION: &str = "3.1";
/// Triangles of one received mesh drawn as polylines; the rest is reported as left out.
const MAX_MESH_TRIANGLES: usize = 5_000;

#[derive(Default)]
struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
    any: bool,
}

impl Bounds {
    fn add(&mut self, x: f64, y: f64, z: f64) {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return;
        }
        if !self.any {
            self.min = [x, y, z];
            self.max = [x, y, z];
            self.any = true;
        } else {
            self.min[0] = self.min[0].min(x);
            self.min[1] = self.min[1].min(y);
            self.min[2] = self.min[2].min(z);
            self.max[0] = self.max[0].max(x);
            self.max[1] = self.max[1].max(y);
            self.max[2] = self.max[2].max(z);
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "min": { "x": self.min[0], "y": self.min[1], "z": self.min[2] },
            "max": { "x": self.max[0], "y": self.max[1], "z": self.max[2] },
        })
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn flat_points(pts: &[[f64; 3]], bounds: &mut Bounds) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len() * 3);
    for &[x, y, z] in pts {
        bounds.add(x, y, z);
        out.push(round3(x));
        out.push(round3(y));
        out.push(round3(z));
    }
    out
}

fn make_curve(
    handle: &str,
    layer: &str,
    pts: &[[f64; 3]],
    closed: bool,
    curve_type: &str,
    extras: Option<Map<String, Value>>,
    bounds: &mut Bounds,
) -> Value {
    let flat = flat_points(pts, bounds);
    let mut obj = Map::new();
    obj.insert("id".into(), json!(format!("curve_{handle}")));
    obj.insert("sourceObjectIds".into(), json!([handle]));
    obj.insert("sourceObjectNames".into(), json!([handle]));
    obj.insert("layerName".into(), json!(layer));
    obj.insert("type".into(), json!(curve_type));
    obj.insert("closed".into(), json!(closed));
    obj.insert("points".into(), json!(flat));
    if let Some(extra) = extras {
        for (k, v) in extra {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

fn sample_arc(cx: f64, cy: f64, cz: f64, r: f64, start: f64, end: f64, segments: usize) -> Vec<[f64; 3]> {
    let mut sweep = end - start;
    if sweep <= 0.0 {
        sweep += TAU;
    }
    let steps = segments.max(8);
    (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f64 / steps as f64);
            [cx + r * a.cos(), cy + r * a.sin(), cz]
        })
        .collect()
}

fn resolve_entities<'a>(app: &'a CadApp, p: &Value) -> Result<(Vec<&'a Entity>, usize), String> {
    let state = app.session.state().map_err(|e| e.to_string())?;
    let doc = &state.doc;
    let scope = p.get("scope").and_then(Value::as_str).unwrap_or("selection-or-all");

    let explicit_ids: Vec<Handle> = p
        .get("ids")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .filter_map(Handle::parse_hex)
                .collect()
        })
        .unwrap_or_default();

    let handles: Vec<Handle> = if !explicit_ids.is_empty() {
        explicit_ids
    } else if scope == "all" {
        doc.model.iter().map(|e| e.handle).collect()
    } else if !state.selection.is_empty() {
        state.selection.clone()
    } else if scope == "selection-only" {
        return Err("NO_SELECTION: PAULGA CAD에서 내보낼 객체를 먼저 선택해 주세요".into());
    } else {
        doc.model.iter().map(|e| e.handle).collect()
    };

    let selected_count = handles.len();
    let entities: Vec<&Entity> = handles
        .into_iter()
        .filter_map(|h| doc.model.get(h).map(std::sync::Arc::as_ref))
        .collect();
    Ok((entities, selected_count))
}

pub fn export_selected_mesh(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let (entities, asked_count) = resolve_entities(app, p)?;
    if entities.is_empty() {
        return Err("NO_SELECTION: PAULGA CAD에 내보낼 객체가 없습니다".into());
    }
    // Selected means found in the drawing: a handle that is not there is never counted as sent, so
    // Studio refuses the transfer before anything reaches the target.
    let selected_count = entities.len();
    let missing_count = asked_count.saturating_sub(selected_count);

    let doc_name = app
        .session
        .state()
        .ok()
        .and_then(|s| s.path.as_deref().and_then(|p| Path::new(p).file_name()).map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Drawing.dwg".into());

    let mut meshes: Vec<Value> = Vec::new();
    let mut curves: Vec<Value> = Vec::new();
    let mut points: Vec<Value> = Vec::new();
    let mut texts: Vec<Value> = Vec::new();
    let mut bounds = Bounds::default();
    let mut vertex_count = 0usize;
    let mut triangle_count = 0usize;
    let mut exported_count = 0usize;

    for entity in &entities {
        let handle = entity.handle.hex();
        let layer = entity.common.layer.as_str();
        let before = meshes.len() + curves.len() + points.len() + texts.len();

        match &entity.kind {
            EntityKind::Line(l) => {
                let pts = [[l.a.x, l.a.y, l.a.z], [l.b.x, l.b.y, l.b.z]];
                curves.push(make_curve(&handle, layer, &pts, false, "polyline", None, &mut bounds));
            }
            EntityKind::Point(pt) => {
                bounds.add(pt.p.x, pt.p.y, pt.p.z);
                points.push(json!({
                    "id": format!("point_{handle}"),
                    "sourceObjectIds": [handle],
                    "sourceObjectNames": [handle],
                    "layerName": layer,
                    "position": { "x": round3(pt.p.x), "y": round3(pt.p.y), "z": round3(pt.p.z) },
                }));
            }
            EntityKind::Circle(c) => {
                let pts = sample_arc(c.center.x, c.center.y, c.center.z, c.radius, 0.0, TAU, 36);
                let mut extras = Map::new();
                extras.insert("center".into(), json!({ "x": round3(c.center.x), "y": round3(c.center.y), "z": round3(c.center.z) }));
                extras.insert("radius".into(), json!(round3(c.radius)));
                curves.push(make_curve(&handle, layer, &pts, true, "circle", Some(extras), &mut bounds));
            }
            EntityKind::Arc(a) => {
                let pts = sample_arc(a.center.x, a.center.y, a.center.z, a.radius, a.start, a.end, 24);
                let mut extras = Map::new();
                extras.insert("center".into(), json!({ "x": round3(a.center.x), "y": round3(a.center.y), "z": round3(a.center.z) }));
                extras.insert("radius".into(), json!(round3(a.radius)));
                extras.insert("startAngle".into(), json!(a.start));
                extras.insert("endAngle".into(), json!(a.end));
                curves.push(make_curve(&handle, layer, &pts, false, "arc", Some(extras), &mut bounds));
            }
            EntityKind::Ellipse(e) => {
                let steps = 36usize;
                let mut sweep = e.end - e.start;
                if sweep <= 0.0 {
                    sweep += TAU;
                }
                let closed = (sweep - TAU).abs() < 1e-4;
                let maj = e.major;
                let min_x = -maj.y * e.ratio;
                let min_y = maj.x * e.ratio;
                let pts: Vec<[f64; 3]> = (0..=steps)
                    .map(|i| {
                        let t = e.start + sweep * (i as f64 / steps as f64);
                        [
                            e.center.x + maj.x * t.cos() + min_x * t.sin(),
                            e.center.y + maj.y * t.cos() + min_y * t.sin(),
                            e.center.z,
                        ]
                    })
                    .collect();
                curves.push(make_curve(&handle, layer, &pts, closed, "ellipse", None, &mut bounds));
            }
            EntityKind::LwPolyline(pl) => {
                if pl.vertices.len() >= 2 {
                    let pts: Vec<[f64; 3]> = pl.vertices.iter().map(|v| [v.p.x, v.p.y, pl.elevation]).collect();
                    curves.push(make_curve(&handle, layer, &pts, pl.closed, "polyline", None, &mut bounds));
                }
            }
            EntityKind::Polyline3d(pl) => {
                if pl.points.len() >= 2 {
                    let pts: Vec<[f64; 3]> = pl.points.iter().map(|v| [v.x, v.y, v.z]).collect();
                    curves.push(make_curve(&handle, layer, &pts, pl.closed, "polyline", None, &mut bounds));
                }
            }
            EntityKind::Spline(sp) => {
                let pts: Vec<[f64; 3]> = if !sp.fit.is_empty() {
                    sp.fit.iter().map(|v| [v.x, v.y, 0.0]).collect()
                } else {
                    sp.control.iter().map(|v| [v.x, v.y, 0.0]).collect()
                };
                if pts.len() >= 2 {
                    curves.push(make_curve(&handle, layer, &pts, sp.closed, "spline", None, &mut bounds));
                }
            }
            EntityKind::Text(t) => {
                bounds.add(t.insert.x, t.insert.y, t.insert.z);
                texts.push(json!({
                    "id": format!("text_{handle}"),
                    "sourceObjectIds": [handle],
                    "layerName": layer,
                    "text": t.value,
                    "position": { "x": round3(t.insert.x), "y": round3(t.insert.y), "z": round3(t.insert.z) },
                    "height": round3(t.height),
                    "rotation": t.rotation,
                }));
            }
            EntityKind::MText(t) => {
                bounds.add(t.insert.x, t.insert.y, t.insert.z);
                texts.push(json!({
                    "id": format!("text_{handle}"),
                    "sourceObjectIds": [handle],
                    "layerName": layer,
                    "text": t.contents,
                    "position": { "x": round3(t.insert.x), "y": round3(t.insert.y), "z": round3(t.insert.z) },
                    "height": round3(t.height),
                    "rotation": t.rotation,
                }));
            }
            EntityKind::Face3d(f) => {
                let mut verts = Vec::with_capacity(12);
                for c in &f.corners {
                    bounds.add(c.x, c.y, c.z);
                    verts.extend_from_slice(&[round3(c.x), round3(c.y), round3(c.z)]);
                }
                let indices: Vec<u32> = if f.corners[2] == f.corners[3] { vec![0, 1, 2] } else { vec![0, 1, 2, 0, 2, 3] };
                vertex_count += 4;
                triangle_count += indices.len() / 3;
                meshes.push(json!({
                    "id": format!("mesh_{handle}"),
                    "sourceObjectIds": [handle],
                    "sourceObjectNames": [handle],
                    "layerName": layer,
                    "vertices": verts,
                    "faces": indices,
                }));
            }
            EntityKind::Solid(s) | EntityKind::Trace(s) => {
                let mut verts = Vec::with_capacity(12);
                for c in &s.corners {
                    bounds.add(c.x, c.y, c.z);
                    verts.extend_from_slice(&[round3(c.x), round3(c.y), round3(c.z)]);
                }
                vertex_count += 4;
                triangle_count += 2;
                meshes.push(json!({
                    "id": format!("mesh_{handle}"),
                    "sourceObjectIds": [handle],
                    "sourceObjectNames": [handle],
                    "layerName": layer,
                    "vertices": verts,
                    "faces": [0, 1, 3, 0, 3, 2],
                }));
            }
            EntityKind::PolyfaceMesh(pm) => {
                let mut verts = Vec::with_capacity(pm.vertices.len() * 3);
                for v in &pm.vertices {
                    bounds.add(v.x, v.y, v.z);
                    verts.extend_from_slice(&[round3(v.x), round3(v.y), round3(v.z)]);
                }
                let mut indices = Vec::new();
                for f in &pm.faces {
                    let idxs: Vec<u32> = f
                        .iter()
                        .take_while(|&&ix| ix != 0)
                        .filter_map(|&ix| ix.unsigned_abs().checked_sub(1))
                        .filter(|&i| (i as usize) < pm.vertices.len())
                        .collect();
                    if idxs.len() >= 3 {
                        indices.extend_from_slice(&[idxs[0], idxs[1], idxs[2]]);
                    }
                    if idxs.len() >= 4 {
                        indices.extend_from_slice(&[idxs[0], idxs[2], idxs[3]]);
                    }
                }
                if !indices.is_empty() {
                    vertex_count += pm.vertices.len();
                    triangle_count += indices.len() / 3;
                    meshes.push(json!({
                        "id": format!("mesh_{handle}"),
                        "sourceObjectIds": [handle],
                        "sourceObjectNames": [handle],
                        "layerName": layer,
                        "vertices": verts,
                        "faces": indices,
                    }));
                }
            }
            _ => {}
        }

        if meshes.len() + curves.len() + points.len() + texts.len() > before {
            exported_count += 1;
        }
    }

    let stats = json!({
        "meshCount": meshes.len(),
        "objectCount": entities.len(),
        "vertexCount": vertex_count,
        "faceCount": triangle_count,
        "triangleCount": triangle_count,
        "curveCount": curves.len(),
        "pointCount": points.len(),
        "textCount": texts.len(),
    });

    let mut bundle = json!({
        "schemaVersion": SCHEMA_VERSION,
        "units": "mm",
        "source": {
            "platform": super::PLATFORM,
            "documentName": doc_name,
        },
        "selectedCount": selected_count,
        "exportedCount": exported_count,
        "meshes": meshes,
        "curves": curves,
        "points": points,
        "instances": [],
        "definitions": {},
        "texts": texts,
        "hatches": [],
        "stats": stats,
    });
    if bounds.any {
        bundle["bounds"] = bounds.to_json();
    }

    // Objects found that carry nothing the bundle can hold (blocks, hatches, dimensions, images…).
    let unsupported = selected_count - exported_count;
    let mut result = json!({
        "selectedElementCount": selected_count,
        "selectedCount": selected_count,
        "exportedCount": exported_count,
        "missingHandleCount": missing_count,
        "skipped": skipped_json(&[("unsupported", unsupported)], &[]),
        "stats": stats,
    });
    if bounds.any {
        result["bounds"] = bounds.to_json();
    }

    if let Some(path) = p.get("bundleFilePath").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        let bytes = serde_json::to_vec(&bundle).map_err(|e| format!("bundle serialize failed: {e}"))?;
        if let Some(parent) = Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, &bytes).map_err(|e| format!("failed to write bundle file {path}: {e}"))?;
        result["bundleFilePath"] = json!(path);
        result["bundleSizeBytes"] = json!(bytes.len());
    } else {
        result["bundle"] = bundle;
    }

    Ok(result)
}

/// The shared transfer report's `skipped` block (Studio's transfer-report.ts): whole objects left
/// out by reason, and objects that went in only in part by reason.
fn skipped_json(objects: &[(&str, usize)], pieces: &[(&str, usize)]) -> Value {
    let by = |list: &[(&str, usize)]| list.iter().filter(|(_, n)| *n > 0).map(|(reason, n)| (reason.to_string(), json!(n))).collect::<Map<String, Value>>();
    json!({
        "count": objects.iter().map(|(_, n)| n).sum::<usize>(),
        "byReason": by(objects),
        "partialPieces": pieces.iter().map(|(_, n)| n).sum::<usize>(),
        "piecesByReason": by(pieces),
    })
}

/// Bundle entries no receiver here can make: block instances and hatches.
fn unsupported_entries(bundle: &Value) -> usize {
    ["instances", "hatches"].iter().filter_map(|key| bundle.get(*key).and_then(Value::as_array)).map(Vec::len).sum()
}

/// A mesh's triangle indices: `faces` in the shared bundle (AutoCAD, Rhino, SketchUp, Revit, Blender and
/// Studio), or `indices` from a PAULGA app from before it wrote `faces` — it wrote and read only
/// `indices`, so its meshes reached no other program and theirs never came in.
fn mesh_faces(mesh: &Value) -> Option<&Vec<Value>> {
    mesh.get("faces").or_else(|| mesh.get("indices")).and_then(Value::as_array)
}

fn load_bundles(p: &Value) -> Result<Vec<Value>, String> {
    if let Some(bundle) = p.get("bundle").filter(|v| v.is_object()) {
        return Ok(vec![bundle.clone()]);
    }
    let mut paths: Vec<String> = Vec::new();
    if let Some(path) = p.get("bundleFilePath").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        paths.push(path.to_string());
    }
    if let Some(arr) = p.get("bundleFilePaths").and_then(Value::as_array) {
        for item in arr {
            if let Some(s) = item.as_str().filter(|s| !s.trim().is_empty()) {
                paths.push(s.to_string());
            }
        }
    }
    if paths.is_empty() {
        return Err("import-mesh-bundle needs `bundle` or `bundleFilePath`".into());
    }
    let mut bundles = Vec::with_capacity(paths.len());
    for path in paths {
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("failed to read bundle file {path}: {e}"))?;
        let parsed: Value = serde_json::from_str(&raw).map_err(|e| format!("invalid bundle JSON in {path}: {e}"))?;
        bundles.push(parsed);
    }
    Ok(bundles)
}

fn parse_curve_points(val: &Value) -> Vec<[f64; 2]> {
    let Some(arr) = val.as_array() else { return Vec::new() };
    if arr.is_empty() {
        return Vec::new();
    }
    // Flat [x0, y0, z0, x1, y1, z1, ...]
    if arr[0].is_number() {
        return arr
            .chunks_exact(3)
            .filter_map(|c| Some([c[0].as_f64()?, c[1].as_f64()?]))
            .collect();
    }
    // Nested [[x, y, z?], ...] or [{x, y, z?}, ...]
    arr.iter()
        .filter_map(|item| {
            if let Some(a) = item.as_array() {
                Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
            } else if let Some(o) = item.as_object() {
                Some([o.get("x")?.as_f64()?, o.get("y")?.as_f64()?])
            } else {
                None
            }
        })
        .collect()
}

fn parse_point_2d(val: &Value) -> Option<[f64; 2]> {
    if let Some(o) = val.as_object() {
        return Some([o.get("x")?.as_f64()?, o.get("y")?.as_f64()?]);
    }
    if let Some(a) = val.as_array() {
        return Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?]);
    }
    None
}

fn ensure_layer(app: &mut CadApp, known: &mut HashSet<String>, name: &str) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = trimmed.to_ascii_lowercase();
    if !known.contains(&key) {
        let _ = app.run("layer.new", json!({ "name": trimmed }));
        known.insert(key);
    }
    let _ = app.run("layer.set", json!({ "name": trimmed, "current": true }));
}

pub fn import_mesh_bundle(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let bundles = load_bundles(p)?;
    let target_layer = p.get("targetLayer").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());

    let mut known_layers: HashSet<String> = app
        .run("layer", json!({}))
        .ok()
        .and_then(|v| v.get("layers").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|l| l.get("name").and_then(Value::as_str).map(|s| s.to_ascii_lowercase()))
        .collect();

    let prev_layer = app
        .session
        .doc()
        .ok()
        .map(|d| d.header.str("CLAYER", "0").to_string());

    let (mut curve_count, mut mesh_count, mut point_count, mut text_count) = (0usize, 0usize, 0usize, 0usize);
    // What the bundle held, and why what did not go in stayed out (the transfer report's counts).
    let mut given = 0usize;
    let (mut unsupported, mut empty, mut failed) = (0usize, 0usize, 0usize);
    let (mut over_limit, mut part_failed) = (0usize, 0usize);

    app.run("undo", json!({ "mode": "begin" }))?;

    for bundle in &bundles {
        let skipped_kinds = unsupported_entries(bundle);
        given += skipped_kinds;
        unsupported += skipped_kinds;
        if let Some(curves) = bundle.get("curves").and_then(Value::as_array) {
            for curve in curves {
                given += 1;
                let layer = target_layer.or_else(|| curve.get("layerName").and_then(Value::as_str)).unwrap_or("0");
                ensure_layer(app, &mut known_layers, layer);

                let kind = curve.get("type").and_then(Value::as_str).unwrap_or("polyline");
                if kind == "circle"
                    && let (Some(center), Some(radius)) = (
                        curve.get("center").and_then(parse_point_2d),
                        curve.get("radius").and_then(Value::as_f64).filter(|r| *r > 1e-9),
                    )
                {
                    if app.run("circle", json!({ "center": center, "radius": radius })).is_ok() {
                        curve_count += 1;
                        continue;
                    }
                }

                let pts = curve.get("points").map(parse_curve_points).unwrap_or_default();
                let closed = curve.get("closed").and_then(Value::as_bool).unwrap_or(false);
                let made = if pts.len() == 2 && !closed {
                    app.run("line", json!({ "points": pts })).is_ok()
                } else if pts.len() >= 2 {
                    app.run("pline", json!({ "vertices": pts, "closed": closed })).is_ok()
                } else {
                    empty += 1;
                    continue;
                };
                if made {
                    curve_count += 1;
                } else {
                    failed += 1;
                }
            }
        }

        // 3D meshes from Rhino / SketchUp / Revit / Blender: project each triangle into 2D polylines.
        if let Some(meshes) = bundle.get("meshes").and_then(Value::as_array) {
            for mesh in meshes {
                given += 1;
                let layer = target_layer.or_else(|| mesh.get("layerName").and_then(Value::as_str)).unwrap_or("0");
                ensure_layer(app, &mut known_layers, layer);

                let (Some(verts_arr), Some(idx_arr)) =
                    (mesh.get("vertices").and_then(Value::as_array), mesh_faces(mesh))
                else {
                    empty += 1;
                    continue;
                };
                let verts: Vec<[f64; 2]> = verts_arr
                    .chunks_exact(3)
                    .filter_map(|c| Some([c[0].as_f64()?, c[1].as_f64()?]))
                    .collect();
                let indices: Vec<usize> = idx_arr.iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect();
                let (mut drawn, mut refused) = (0usize, 0usize);
                for tri in indices.chunks_exact(3).take(MAX_MESH_TRIANGLES) {
                    let (Some(&a), Some(&b), Some(&c)) = (verts.get(tri[0]), verts.get(tri[1]), verts.get(tri[2])) else {
                        continue;
                    };
                    if app.run("pline", json!({ "vertices": [a, b, c], "closed": true })).is_ok() {
                        drawn += 1;
                    } else {
                        refused += 1;
                    }
                }
                if drawn == 0 {
                    if refused > 0 { failed += 1 } else { empty += 1 }
                    continue;
                }
                mesh_count += 1;
                // Each triangle is a polyline here: past the limit the rest of the mesh stays out.
                if indices.len() / 3 > MAX_MESH_TRIANGLES {
                    over_limit += 1;
                } else if refused > 0 {
                    part_failed += 1;
                }
            }
        }

        if let Some(points) = bundle.get("points").and_then(Value::as_array) {
            for pt in points {
                given += 1;
                let layer = target_layer.or_else(|| pt.get("layerName").and_then(Value::as_str)).unwrap_or("0");
                ensure_layer(app, &mut known_layers, layer);
                match pt.get("position").and_then(parse_point_2d) {
                    Some(pos) if app.run("point", json!({ "point": pos })).is_ok() => point_count += 1,
                    Some(_) => failed += 1,
                    None => empty += 1,
                }
            }
        }

        if let Some(texts) = bundle.get("texts").and_then(Value::as_array) {
            for txt in texts {
                given += 1;
                let layer = target_layer.or_else(|| txt.get("layerName").and_then(Value::as_str)).unwrap_or("0");
                ensure_layer(app, &mut known_layers, layer);
                let content = txt.get("text").and_then(Value::as_str).unwrap_or("");
                let Some(pos) = txt.get("position").and_then(parse_point_2d).filter(|_| !content.is_empty()) else {
                    empty += 1;
                    continue;
                };
                let height = txt.get("height").and_then(Value::as_f64).unwrap_or(2.5).max(0.1);
                // The bundle's rotation is in radians (AutoCAD's, and this program's own export); the
                // `text` command takes `at` and degrees. It was given `position`, so no text ever came in.
                let rotation = txt.get("rotation").and_then(Value::as_f64).unwrap_or(0.0).to_degrees();
                if app.run("text", json!({ "at": pos, "text": content, "height": height, "rotation": rotation })).is_ok() {
                    text_count += 1;
                } else {
                    failed += 1;
                }
            }
        }
    }

    if let Some(prev) = prev_layer {
        let _ = app.run("layer.set", json!({ "name": prev, "current": true }));
    }
    app.run("undo", json!({ "mode": "end" }))?;

    let imported_count = curve_count + mesh_count + point_count + text_count;
    Ok(json!({
        "sourceCount": given,
        "importedCount": imported_count,
        "createdCount": imported_count,
        "skipped": skipped_json(
            &[("unsupported", unsupported), ("empty", empty), ("create-failed", failed)],
            &[("limit", over_limit), ("create-failed", part_failed)],
        ),
        "curveCount": curve_count,
        "meshCount": mesh_count,
        "pointCount": point_count,
        "textCount": text_count,
    }))
}
