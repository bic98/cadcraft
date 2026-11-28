//! The tools PAULGA Studio runs in this program. Each one is a thin face on registered commands,
//! so Studio can do nothing the command line cannot, and every change is an ordinary undo step.
//!
//! [`call`] is the single entry: the control method `studio.tool` and the command of the same
//! name both go through it, with the same checks.

use serde_json::{Map, Value, json};

use crate::CadApp;

/// What a tool does to the drawing; Studio asks its operator before a `Change`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Change,
    /// Runs code Studio sent. Only with the authorization Studio's bridge puts on that one
    /// request ([`authorize`]), never from a parameter.
    Script,
}

pub struct Tool {
    pub name: &'static str,
    pub access: Access,
    pub domain: &'static str,
    pub risk: &'static str,
    pub description: &'static str,
    /// The parameters as JSON Schema properties: `(name, type, description)`.
    pub params: &'static [(&'static str, &'static str, &'static str)],
}

const fn read(name: &'static str, domain: &'static str, description: &'static str, params: &'static [(&'static str, &'static str, &'static str)]) -> Tool {
    Tool { name, access: Access::Read, domain, risk: "low", description, params }
}

const fn change(
    name: &'static str,
    domain: &'static str,
    risk: &'static str,
    description: &'static str,
    params: &'static [(&'static str, &'static str, &'static str)],
) -> Tool {
    Tool { name, access: Access::Change, domain, risk, description, params }
}

/// Every tool, in the order `get-capabilities` lists them. Studio's allowlist for this program
/// (`CadPlatform::PaulgaCad` in its `cad/protocol.rs`) names exactly these.
pub const TOOLS: &[Tool] = &[
    read("get-capabilities", "system", "List the tools PAULGA CAD runs for PAULGA Studio.", &[]),
    read("get-context", "document", "The open drawings and the active one: name, path, space, current layer, extents, view, object counts.", &[]),
    read(
        "inspect-drawing",
        "document",
        "Full summary of the active drawing: entity counts, extents, layers, styles, blocks, layouts, selection, undo history.",
        &[("entities", "boolean", "Include entities with handles and geometry (default false)."), ("limit", "integer", "Most entities returned.")],
    ),
    read(
        "find-text",
        "data",
        "Find texts in the drawing (TEXT, MTEXT, block attributes, multileaders) filtered by substring, layer, or window.",
        &[
            ("contains", "string", "Case-insensitive substring filter."),
            ("layer", "string", "Optional layer filter."),
            ("window", "array", "Optional [[x0,y0],[x1,y1]] window in drawing units."),
            ("limit", "integer", "Page size (default 200)."),
            ("offset", "integer", "Items to skip."),
        ],
    ),
    read(
        "get-selection",
        "object",
        "Handles of the selected objects; with `geometry`, their layers and geometry too.",
        &[("geometry", "boolean", "Also return each object's layer and geometry (default false)."), ("limit", "integer", "Most objects returned with geometry (default 200).")],
    ),
    read(
        "query-entities",
        "data",
        "Objects with handles, layers and geometry, a page at a time.",
        &[
            ("type", "string", "Only this object type (Line, Circle, Polyline, Text…)."),
            ("layer", "string", "Only this layer."),
            ("selected", "boolean", "Only the selected objects."),
            ("window", "array", "[[x0,y0],[x1,y1]] in drawing units."),
            ("crossing", "boolean", "With `window`: objects that touch it (default) or lie inside it (false)."),
            ("limit", "integer", "Page size (default 200, at most 5000)."),
            ("offset", "integer", "Objects to skip."),
        ],
    ),
    read(
        "query-buckets",
        "layer",
        "The drawing's layers as buckets: id `layer:<name>`, name, colour, whether shown.",
        &[("counts", "boolean", "Also count each layer's objects (default false; slower in a large drawing).")],
    ),
    read(
        "list-commands",
        "system",
        "PAULGA CAD's command catalog for `run-command`: id, label, aliases and the JSON parameters each takes.",
        &[("filter", "string", "Only commands whose id, label or alias contains this text."), ("limit", "integer", "Most commands returned (default 60).")],
    ),
    read(
        "capture-view",
        "view",
        "A PNG of the drawing: the current view or the whole drawing.",
        &[
            ("mode", "string", "`current` (default) or `whole`."),
            ("longEdgePx", "integer", "Long edge in pixels, 256 to 8192 (default 2560)."),
            ("background", "string", "`white` (default), `dark`, `paper` or `#rrggbb`."),
        ],
    ),
    change(
        "restore-view",
        "view",
        "low",
        "Move the view to a `viewState` that `capture-view` or `frame-objects` returned.",
        &[("viewState", "object", "{center: [x,y], height, twist?}.")],
    ),
    change(
        "frame-objects",
        "view",
        "low",
        "Zoom to the given objects, or to the whole drawing. Returns the view it left.",
        &[("ids", "array", "Object handles; without them the whole drawing is framed.")],
    ),
    change(
        "select",
        "object",
        "low",
        "Select objects by handle, or clear the selection.",
        &[
            ("ids", "array", "Object handles to select."),
            ("add", "boolean", "Add to the selection instead of replacing it."),
            ("clear", "boolean", "Clear the selection."),
        ],
    ),
    change(
        "apply-layer-visibility",
        "display",
        "medium",
        "Show or hide layers, as one undo step. A layer shown is turned on and thawed.",
        &[("states", "array", "[{name, visible}] for the layers to change."), ("showAll", "boolean", "Show every layer.")],
    ),
    change(
        "new-drawing",
        "document",
        "medium",
        "Create a new drawing (metric or imperial).",
        &[("metric", "boolean", "Use metric units (default true).")],
    ),
    change(
        "open-document",
        "document",
        "medium",
        "Open a DXF or DWG drawing from disk.",
        &[("path", "string", "Absolute path to the DXF/DWG file.")],
    ),
    change(
        "save-document",
        "document",
        "medium",
        "Save the drawing (DXF/DWG) or export to SVG/PNG/PDF by extension.",
        &[("path", "string", "Destination file path.")],
    ),
    change(
        "place-image",
        "object",
        "medium",
        "Attach/place a raster image (PNG/JPEG/BMP/TIFF) into the drawing (`imageattach`).",
        &[
            ("path", "string", "Absolute path to the image file."),
            ("at", "array", "Lower-left insertion point [x, y] (default [0, 0])."),
            ("scale", "number", "Scale factor (default 1)."),
            ("width", "number", "Optional width in drawing units (instead of `scale`)."),
            ("rotation", "number", "Optional rotation in degrees."),
            ("name", "string", "Optional image reference name."),
        ],
    ),
    change(
        "command-line",
        "system",
        "high",
        "Run one AutoCAD-style command line input, e.g. `line 0,0 10,0 10,5 c`, `circle 5,2.5 1`, `zoom e`.",
        &[("text", "string", "Command-line input text.")],
    ),
    change(
        "run-batch",
        "system",
        "high",
        "Run a multi-line AutoCAD `.scr` command script (one or more inputs per line) or JSON `steps` array.",
        &[
            ("text", "string", "Multi-line `.scr` command text."),
            ("steps", "array", "Alternative: array of `{command, params}` objects."),
        ],
    ),
    change(
        "run-command",
        "system",
        "high",
        "Run one PAULGA CAD command with JSON parameters (see `list-commands`). Never opens a dialog. Commands that run scripts or LISP, or quit the program, are refused.",
        &[("command", "string", "Command id or alias, e.g. `line`, `circle`, `layer.new`."), ("params", "object", "The command's parameters.")],
    ),
    change("undo", "system", "medium", "Undo the last change, or the last `count` changes.", &[("count", "integer", "Steps to undo (default 1).")]),
    change("redo", "system", "medium", "Redo the last undone change, or the last `count` changes.", &[("count", "integer", "Steps to redo (default 1).")]),
    change("cancel", "system", "low", "Cancel the active command or clear the selection (Escape).", &[]),
    read(
        "export-selected-mesh",
        "object",
        "Export selected objects (or the whole drawing) as a PAULGA Send Mode mesh bundle.",
        &[
            ("scope", "string", "`selection-only`, `selection-or-all` (default), or `all`."),
            ("ids", "array", "Optional object handles to export directly."),
            ("bundleFilePath", "string", "Optional file path to write the bundle JSON to."),
        ],
    ),
    change(
        "import-mesh-bundle",
        "object",
        "medium",
        "Import a PAULGA Send Mode mesh bundle (curves, meshes, points, texts) into the open drawing as one undo step.",
        &[
            ("bundle", "object", "Inline Send Mode bundle JSON."),
            ("bundleFilePath", "string", "Path to a Send Mode bundle JSON file."),
            ("bundleFilePaths", "array", "Chunked Send Mode bundle JSON file paths."),
            ("targetLayer", "string", "Optional layer name to place imported objects on."),
        ],
    ),
    Tool {
        name: "execute-script",
        access: Access::Script,
        domain: "system",
        risk: "high",
        description: "Run AutoLISP in the open drawing, as one undo step; a script that fails is undone. It reads and writes files only in PAULGA CAD's scripts folder, stays in its drawing, and leaves no functions or variables behind. Needs script execution switched on in PAULGA Studio.",
        params: &[
            ("code", "string", "AutoLISP source, at most 16000 characters. (command …) runs PAULGA CAD commands."),
            ("inputs", "array", "Answers to the script's get… calls, in order: numbers, strings, [x, y] points."),
        ],
    },
];

/// Commands `run-command` does not run: they execute code or files the person did not see, or end
/// the session. Compared after aliases are resolved.
const REFUSED: &[&str] = &["script", "lisp", "appload", "ui.quit", "studio.tool", "studio.connect", "studio.disconnect"];

/// The longest script, in characters (Studio's own limit for generated code).
const MAX_SCRIPT_CHARS: usize = 16_000;
/// The furthest ahead an authorization may expire.
const MAX_AUTHORIZATION_MS: u64 = 5 * 60 * 1000;

const DEFAULT_PAGE: u64 = 200;
const MAX_PAGE: u64 = 5000;

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|tool| tool.name == name)
}

/// What `get-capabilities` answers: one descriptor per tool.
pub fn capabilities() -> Value {
    let operations: Vec<Value> = TOOLS
        .iter()
        .map(|tool| {
            let verb = tool.name.replace('-', "_");
            let properties: Map<String, Value> =
                tool.params.iter().map(|(name, kind, description)| (name.to_string(), json!({ "type": kind, "description": description }))).collect();
            json!({
                "toolName": tool.name,
                "action": format!("cad.{verb}"),
                "domain": tool.domain,
                "verb": verb,
                "description": tool.description,
                "risk": tool.risk,
                "access": match tool.access {
                    Access::Read => "read",
                    Access::Change => "change",
                    Access::Script => "script",
                },
                "requiresScriptAuthorization": tool.access == Access::Script,
                "paramSchema": { "type": "object", "properties": properties },
            })
        })
        .collect();
    json!({
        "platform": super::PLATFORM,
        "displayName": "PAULGA CAD",
        "pluginVersion": env!("CARGO_PKG_VERSION"),
        "cadVersion": cadcraft_engine::build_version(),
        "units": "drawing units",
        "supportsDocumentFence": true,
        "operations": operations,
    })
}

/// Run the tool `params` names: `{name | toolName, parameters?, expectedRuntimeDocumentId?}`.
///
/// A request meant for another drawing than the open one is refused before anything runs, and so
/// is a change while a command waits for the person's input.
pub fn call(app: &mut CadApp, ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let name = params.get("name").or_else(|| params.get("toolName")).and_then(Value::as_str).ok_or("missing tool `name`")?;
    let tool = find(name).ok_or_else(|| format!("UNSUPPORTED_TOOL: PAULGA CAD has no tool named `{name}`; get-capabilities lists them"))?;
    let empty = json!({});
    let parameters = match params.get("parameters") {
        None | Some(Value::Null) => &empty,
        Some(p) if p.is_object() => p,
        Some(_) => return Err("tool `parameters` must be an object".into()),
    };
    super::publish(app);
    if let (Some(expected), Some(link)) = (params.get("expectedRuntimeDocumentId").and_then(Value::as_str), &app.studio) {
        let open = link.snapshot().document.runtime_id;
        if open != expected {
            return Err(format!("DOCUMENT_FENCE_MISMATCH: the request was for drawing {expected}, and PAULGA CAD now shows {open}"));
        }
    }
    if tool.access == Access::Script {
        let open = app.studio.as_ref().map(|link| link.snapshot().document.runtime_id);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        authorize(params, open.as_deref(), now)?;
    }
    if tool.access != Access::Read
        && let Some(running) = &app.session.running
    {
        return Err(format!("CAD_BUSY: the {} command is waiting for input in PAULGA CAD; finish or cancel it first", running.id.to_uppercase()));
    }
    let result = run(app, ctx, tool.name, parameters)?;
    if tool.access != Access::Read {
        // The change did not come from this window: say so where the person looks.
        app.set_status(crate::tf!("PAULGA Studio ran {tool}", tool = tool.name));
    }
    Ok(result)
}

fn run(app: &mut CadApp, ctx: &egui::Context, name: &str, p: &Value) -> Result<Value, String> {
    match name {
        "get-capabilities" => Ok(capabilities()),
        "get-context" => Ok(context(app)),
        "inspect-drawing" => app.run("drawing.inspect", p.clone()),
        "find-text" => app.run("texts", p.clone()),
        "get-selection" => selection(app, p),
        "query-entities" => entities(app, p),
        "query-buckets" => buckets(app, p),
        "list-commands" => Ok(commands(app, p)),
        "capture-view" => capture(app, ctx, p),
        "restore-view" => restore_view(app, p),
        "frame-objects" => frame(app, p),
        "select" => select(app, p),
        "apply-layer-visibility" => layer_visibility(app, p),
        "new-drawing" => app.run("new", p.clone()),
        "open-document" => crate::control::call(app, ctx, "app.open", p.clone()),
        "save-document" => crate::control::call(app, ctx, "app.save", p.clone()),
        "place-image" => app.run("imageattach", p.clone()),
        "command-line" => crate::control::call(app, ctx, "cmdline.input", p.clone()),
        "run-batch" => run_batch(app, ctx, p),
        "run-command" => run_command(app, p),
        "undo" => app.run("undo", json!({ "count": p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) })),
        "redo" => app.run("redo", json!({ "count": p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) })),
        "cancel" => crate::control::call(app, ctx, "cmdline.key", json!({ "key": "escape" })),
        "export-selected-mesh" => super::exchange::export_selected_mesh(app, p),
        "import-mesh-bundle" => super::exchange::import_mesh_bundle(app, p),
        "execute-script" => script(app, p),
        other => Err(format!("UNSUPPORTED_TOOL: {other}")),
    }
}

fn run_batch(app: &mut CadApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    if let Some(steps) = p.get("steps").and_then(Value::as_array) {
        let mut results = Vec::new();
        for (idx, step) in steps.iter().take(100).enumerate() {
            match run_command(app, step) {
                Ok(res) => results.push(res),
                Err(error) => return Ok(json!({ "completed": idx, "failed": { "step": idx, "error": error }, "results": results })),
            }
        }
        return Ok(json!({ "completed": results.len(), "failed": Value::Null, "results": results }));
    }
    crate::control::call(app, ctx, "cmdline.script", p.clone())
}

/// Whether the request may run a script: `scriptAuthorization` must be Studio's, for this very
/// request (`requestId`), this connection (`clientId`) and the drawing that is open (`open`),
/// and not expired. The host puts `requestId` and `clientId` on the request from the connection
/// itself, so tool parameters cannot grant it.
pub fn authorize(request: &Value, open: Option<&str>, now_unix_ms: u64) -> Result<(), String> {
    let invalid = |why: &str| Err(format!("SCRIPT_AUTHORIZATION_INVALID: {why}"));
    let Some(auth) = request.get("scriptAuthorization").filter(|a| a.is_object()) else {
        return Err("SCRIPT_AUTHORIZATION_REQUIRED: PAULGA CAD runs a script only when PAULGA Studio authorizes that request; switch script execution on for PAULGA CAD in PAULGA Studio".into());
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if auth.get("enabled") != Some(&Value::Bool(true)) {
        return invalid("it is not enabled");
    }
    let (Some(request_id), Some(client_id), Some(open)) = (text(request, "requestId"), text(request, "clientId"), open) else {
        return invalid("the request does not come from a PAULGA Studio connection");
    };
    if text(auth, "requestId").as_deref() != Some(request_id.as_str()) {
        return invalid("it is for another request");
    }
    if text(auth, "targetClientId").as_deref() != Some(client_id.as_str()) {
        return invalid("it is for another connection");
    }
    if text(auth, "runtimeDocumentId").as_deref() != Some(open) {
        return invalid("it is for another drawing than the open one");
    }
    match auth.get("expiresAtUnixMs").and_then(Value::as_u64) {
        Some(expires) if expires <= now_unix_ms => Err("SCRIPT_AUTHORIZATION_EXPIRED: the authorization ran out before the script started".into()),
        Some(expires) if expires - now_unix_ms > MAX_AUTHORIZATION_MS => invalid("it lasts longer than five minutes"),
        Some(_) => Ok(()),
        None => invalid("it has no expiry"),
    }
}

/// The one folder a script may read and write: `%APPDATA%\@paulga\paulga-cad\scripts`.
fn scripts_folder() -> Result<std::path::PathBuf, String> {
    let unavailable = |why: String| format!("SCRIPT_SANDBOX_UNAVAILABLE: {why}; the script was not run");
    let root = std::env::var_os("APPDATA").filter(|v| !v.is_empty()).ok_or_else(|| unavailable("APPDATA is not set".into()))?;
    let folder = std::path::PathBuf::from(root).join("@paulga").join("paulga-cad").join("scripts");
    std::fs::create_dir_all(&folder).map_err(|e| unavailable(format!("{}: {e}", folder.display())))?;
    Ok(folder)
}

fn revision(app: &CadApp) -> Option<u64> {
    app.session.state().ok().map(|state| state.revision)
}

fn script(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let code = p.get("code").or_else(|| p.get("script")).and_then(Value::as_str).filter(|c| !c.trim().is_empty()).ok_or("execute-script needs `code`")?;
    if code.chars().count() > MAX_SCRIPT_CHARS {
        return Err(format!("SCRIPT_TOO_LONG: a script is at most {MAX_SCRIPT_CHARS} characters"));
    }
    if p.get("language").and_then(Value::as_str).is_some_and(|l| !matches!(l.trim().to_ascii_lowercase().as_str(), "autolisp" | "lisp")) {
        return Err("SCRIPT_LANGUAGE_UNSUPPORTED: PAULGA CAD runs AutoLISP".into());
    }
    let folder = scripts_folder()?;
    let roots = cadcraft_engine::file_access::AutomationRoots::new(Some(&folder), Some(&folder))
        .map_err(|e| format!("SCRIPT_SANDBOX_UNAVAILABLE: {e}; the script was not run"))?;
    let before = revision(app);
    let mut run = json!({ "code": code, "script": true });
    if let Some(inputs) = p.get("inputs").filter(|i| i.is_array()) {
        run["inputs"] = inputs.clone();
    }
    // Every file the script or a command it starts reaches is held to the folder.
    let outcome = cadcraft_engine::file_access::confine(roots.as_ref(), || app.run("lisp", run))?;
    let output = outcome.get("output").cloned().unwrap_or_else(|| json!([]));
    if let Some(error) = outcome.get("error").and_then(Value::as_str) {
        // What it changed before failing is one undo step: take it back.
        let undone = revision(app) != before && app.run("undo", json!({ "count": 1 })).is_ok();
        let lines: Vec<&str> = output.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let tail = lines.get(lines.len().saturating_sub(6)..).unwrap_or_default().join(" | ");
        return Err(format!(
            "SCRIPT_FAILED: {error}. {} Last output: {}",
            if undone { "Its changes were undone." } else { "It changed nothing." },
            if tail.is_empty() { "(none)" } else { &tail }
        ));
    }
    Ok(json!({
        "value": outcome.get("value").cloned().unwrap_or(Value::Null),
        "output": output,
        "changed": revision(app) != before,
        "scriptsFolder": folder.display().to_string(),
    }))
}

fn context(app: &mut CadApp) -> Value {
    let summary = app.session.summary();
    let documents = summary.get("documents").cloned().unwrap_or_else(|| json!([]));
    let runtime_id = app.studio.as_ref().map(|link| link.snapshot().document.runtime_id);
    let mut out = json!({
        "platform": super::PLATFORM,
        "cadVersion": cadcraft_engine::build_version(),
        "documents": documents,
        "document": Value::Null,
        "runtimeDocumentId": runtime_id,
        "selectedCount": summary.get("selection").and_then(Value::as_array).map_or(0, Vec::len),
        "running": summary.get("running").cloned().unwrap_or(Value::Null),
    });
    // No drawing open (the Start tab): the list is empty and `document` stays null.
    if let Ok(Value::Object(mut drawing)) = app.run("drawing.inspect", json!({ "entities": false })) {
        for key in ["entities", "selection", "linetypes", "textStyles", "dimStyles", "blocks", "layers"] {
            drawing.remove(key);
        }
        out["document"] = Value::Object(drawing);
    }
    out
}

fn handles(p: &Value) -> Option<Vec<Value>> {
    ["ids", "handles", "selectedIds"].iter().find_map(|key| p.get(*key).and_then(Value::as_array)).cloned()
}

fn page(p: &Value) -> u64 {
    p.get("limit").and_then(Value::as_u64).unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE)
}

fn selection(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let ids = app.session.summary().get("selection").cloned().unwrap_or_else(|| json!([]));
    let count = ids.as_array().map_or(0, Vec::len);
    let mut out = json!({ "selectedIds": ids, "selectedCount": count });
    if p.get("geometry").and_then(Value::as_bool) == Some(true) && count > 0 {
        let found = app.run("entities", json!({ "selected": true, "limit": page(p) }))?;
        out["entities"] = found.get("entities").cloned().unwrap_or_else(|| json!([]));
        out["returned"] = found.get("returned").cloned().unwrap_or(Value::Null);
    }
    Ok(out)
}

fn entities(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let mut query = Map::new();
    for key in ["type", "layer", "selected", "window", "crossing", "offset"] {
        if let Some(value) = p.get(key).filter(|v| !v.is_null()) {
            query.insert(key.into(), value.clone());
        }
    }
    query.insert("limit".into(), json!(page(p)));
    app.run("entities", Value::Object(query))
}

fn layers(app: &mut CadApp) -> Result<Vec<Value>, String> {
    let listed = app.run("layer", json!({}))?;
    Ok(listed.get("layers").and_then(Value::as_array).cloned().unwrap_or_default())
}

fn shown(layer: &Value) -> bool {
    layer["on"] != false && layer["frozen"] != true
}

fn buckets(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let counts = p.get("counts").and_then(Value::as_bool) == Some(true);
    let mut out = Vec::new();
    for layer in layers(app)? {
        let Some(name) = layer.get("name").and_then(Value::as_str) else { continue };
        let mut bucket = json!({
            "id": format!("layer:{name}"),
            "name": name,
            "kind": "layer",
            "visible": shown(&layer),
            "color": layer.get("colorRgb").cloned().unwrap_or(Value::Null),
            "meta": {
                "on": layer["on"], "frozen": layer["frozen"], "locked": layer["locked"], "current": layer["current"],
                "plot": layer["plot"], "linetype": layer["linetype"], "lineweight": layer["lineweight"], "used": layer["used"],
            },
        });
        if counts {
            bucket["objectCount"] = app.run("entities", json!({ "layer": name, "limit": 1 }))?.get("count").cloned().unwrap_or(Value::Null);
        }
        out.push(bucket);
    }
    Ok(json!({ "kinds": ["layer"], "count": out.len(), "buckets": out }))
}

fn commands(app: &CadApp, p: &Value) -> Value {
    let filter = p.get("filter").and_then(Value::as_str).unwrap_or("").to_lowercase();
    let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(60).clamp(1, 1000) as usize;
    let text = |v: &Value| v.as_str().unwrap_or("").to_lowercase();
    let all = crate::control::all_commands(app);
    let matching: Vec<&Value> = all
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|c| !REFUSED.contains(&text(&c["id"]).as_str()))
                .filter(|c| {
                    filter.is_empty()
                        || text(&c["id"]).contains(&filter)
                        || text(&c["label"]).contains(&filter)
                        || c["aliases"].as_array().is_some_and(|aliases| aliases.iter().any(|a| text(a).contains(&filter)))
                })
                .collect()
        })
        .unwrap_or_default();
    let listed: Vec<Value> =
        matching.iter().take(limit).map(|c| json!({ "id": c["id"], "label": c["label"], "aliases": c["aliases"], "params": c["params"], "enabled": c["enabled"] })).collect();
    json!({ "count": matching.len(), "returned": listed.len(), "commands": listed })
}

fn view_state(app: &mut CadApp) -> Result<Value, String> {
    let view = app.run("view.get", json!({}))?;
    Ok(json!({ "tool": super::PLATFORM, "center": view["center"], "height": view["height"], "twist": view["twist"] }))
}

fn capture(app: &mut CadApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    let whole = match p.get("mode").and_then(Value::as_str).unwrap_or("current") {
        "current" => false,
        "whole" => true,
        other => return Err(format!("capture-view mode must be `current` or `whole`, not `{other}`")),
    };
    let long_edge = p.get("longEdgePx").and_then(Value::as_f64).unwrap_or(2560.0).clamp(256.0, 8192.0);
    // The picture has the drawing area's shape, so `current` shows what the person sees.
    let aspect = app.canvas.rect.filter(|r| r.width() > 1.0 && r.height() > 1.0).map_or(1.6, |r| f64::from(r.width() / r.height())).clamp(0.2, 5.0);
    let (width, height) = if aspect >= 1.0 { (long_edge, long_edge / aspect) } else { (long_edge * aspect, long_edge) };
    let background = p.get("background").and_then(Value::as_str).unwrap_or("white");
    let state = view_state(app)?;
    let rendered =
        crate::control::call(app, ctx, "ui.render", json!({ "width": width.round(), "height": height.round(), "fit": whole, "background": background }))?;
    Ok(json!({
        "image": rendered["pngBase64"],
        "mimeType": "image/png",
        "width": rendered["width"],
        "height": rendered["height"],
        "mode": if whole { "whole" } else { "current" },
        "truncated": rendered["truncated"],
        "viewState": state,
    }))
}

fn restore_view(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let state = p.get("viewState").filter(|v| v.is_object()).unwrap_or(p);
    let (Some(center), Some(height)) = (state.get("center").filter(|c| c.is_array()), state.get("height").and_then(Value::as_f64)) else {
        return Err("restore-view needs a viewState with `center` [x, y] and `height`".into());
    };
    let mut view = json!({ "center": center, "height": height });
    if let Some(twist) = state.get("twist").and_then(Value::as_f64) {
        view["twist"] = json!(twist);
    }
    app.run("view.set", view)?;
    Ok(json!({ "viewState": view_state(app)? }))
}

fn frame(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let previous = view_state(app)?;
    match handles(p).filter(|ids| !ids.is_empty()) {
        Some(ids) => app.run("zoom", json!({ "mode": "object", "handles": ids }))?,
        None => app.run("zoom", json!({ "mode": "extents" }))?,
    };
    Ok(json!({ "previousViewState": previous, "viewState": view_state(app)? }))
}

fn select(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let result = match handles(p) {
        Some(ids) => app.run("select", json!({ "handles": ids, "add": p.get("add").and_then(Value::as_bool).unwrap_or(false) }))?,
        None if p.get("clear").and_then(Value::as_bool) == Some(true) => app.run("select", json!({ "clear": true }))?,
        None => return Err("select needs `ids` (object handles) or `clear: true`".into()),
    };
    let ids = app.session.summary().get("selection").cloned().unwrap_or_else(|| json!([]));
    Ok(json!({ "selectedCount": ids.as_array().map_or(0, Vec::len), "selectedIds": ids, "result": result }))
}

fn layer_visibility(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let existing = layers(app)?;
    let mut wanted: Vec<(String, bool)> = Vec::new();
    if p.get("showAll").and_then(Value::as_bool) == Some(true) {
        wanted.extend(existing.iter().filter_map(|l| l.get("name").and_then(Value::as_str)).map(|name| (name.to_string(), true)));
    }
    for state in p.get("states").and_then(Value::as_array).into_iter().flatten() {
        let name = state.get("name").and_then(Value::as_str).or_else(|| state.get("id").and_then(Value::as_str).and_then(|id| id.strip_prefix("layer:")));
        let (Some(name), Some(visible)) = (name, state.get("visible").and_then(Value::as_bool)) else {
            return Err("each of `states` needs a layer `name` and a boolean `visible`".into());
        };
        wanted.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        wanted.push((name.to_string(), visible));
    }
    if wanted.is_empty() {
        return Err("apply-layer-visibility needs `states` [{name, visible}] or `showAll: true`".into());
    }
    let (mut changed, mut unchanged, mut failed) = (0u32, 0u32, Vec::new());
    app.run("undo", json!({ "mode": "begin" }))?;
    for (name, visible) in &wanted {
        let Some(layer) = existing.iter().find(|l| l.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(name))) else {
            failed.push(json!({ "name": name, "error": "no such layer" }));
            continue;
        };
        if shown(layer) == *visible {
            unchanged += 1;
            continue;
        }
        // Shown means on and thawed; hidden only turns the layer off, so thawing is never needed
        // to bring it back and the current layer (which cannot be frozen) hides like any other.
        let set = if *visible { json!({ "name": name, "on": true, "frozen": false }) } else { json!({ "name": name, "on": false }) };
        match app.run("layer.set", set) {
            Ok(_) => changed += 1,
            Err(error) => failed.push(json!({ "name": name, "error": error })),
        }
    }
    app.run("undo", json!({ "mode": "end" }))?;
    Ok(json!({ "changed": changed, "unchanged": unchanged, "failed": failed }))
}

fn run_command(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let typed = p.get("command").or_else(|| p.get("id")).and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty()).ok_or("run-command needs `command`")?;
    let id = cadcraft_engine::cmd::resolve_alias(typed);
    if REFUSED.contains(&id.as_str()) || REFUSED.contains(&typed.to_ascii_lowercase().as_str()) {
        return Err(format!("COMMAND_REFUSED: PAULGA Studio does not run {} through run-command", id.to_uppercase()));
    }
    let params = match p.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(params) if params.is_object() => params.clone(),
        Some(_) => return Err("run-command `params` must be an object".into()),
    };
    let result = app.run(&id, params)?;
    Ok(json!({ "command": id, "result": result }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Link;
    use cadcraft_engine::Session;

    fn app() -> CadApp {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.run("new", json!({})).unwrap();
        app
    }

    fn tool(app: &mut CadApp, name: &str, parameters: Value) -> Result<Value, String> {
        call(app, &egui::Context::default(), &json!({ "name": name, "parameters": parameters }))
    }

    #[test]
    fn every_listed_tool_is_one_the_dispatcher_runs() {
        let listed: Vec<String> = capabilities()["operations"].as_array().unwrap().iter().map(|o| o["toolName"].as_str().unwrap().to_string()).collect();
        assert_eq!(listed.len(), TOOLS.len());
        let mut app = app();
        for name in &listed {
            let error = tool(&mut app, name, json!({})).err().unwrap_or_default();
            assert!(!error.starts_with("UNSUPPORTED_TOOL"), "{name}: {error}");
        }
        assert!(tool(&mut app, "execute-python", json!({})).unwrap_err().starts_with("UNSUPPORTED_TOOL"));
    }

    fn now() -> u64 {
        u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).unwrap()
    }

    /// A script request as the host forwards it, with Studio's authorization for `document`.
    fn script_request(code: &str, document: &str) -> Value {
        json!({
            "name": "execute-script", "parameters": { "code": code }, "requestId": "r-9", "clientId": "c-1",
            "scriptAuthorization": { "enabled": true, "requestId": "r-9", "targetClientId": "c-1", "runtimeDocumentId": document, "expiresAtUnixMs": now() + 60_000 },
        })
    }

    #[test]
    fn a_script_runs_only_with_studios_authorization_for_this_request_and_drawing() {
        let good = script_request("(+ 1 2)", "doc:1");
        let at = now();
        assert_eq!(authorize(&good, Some("doc:1"), at), Ok(()));
        let with = |edit: fn(&mut Value)| {
            let mut request = script_request("(+ 1 2)", "doc:1");
            edit(&mut request);
            authorize(&request, Some("doc:1"), now()).unwrap_err()
        };
        assert!(with(|r| r["scriptAuthorization"] = Value::Null).starts_with("SCRIPT_AUTHORIZATION_REQUIRED"));
        // An authorization written into the tool's own parameters grants nothing.
        assert!(with(|r| r["parameters"]["scriptAuthorization"] = r["scriptAuthorization"].take()).starts_with("SCRIPT_AUTHORIZATION_REQUIRED"));
        let edits: [fn(&mut Value); 8] = [
            |r| r["scriptAuthorization"]["enabled"] = json!(false),
            |r| r["scriptAuthorization"]["requestId"] = json!("r-8"),
            |r| r["scriptAuthorization"]["targetClientId"] = json!("c-2"),
            |r| r["scriptAuthorization"]["runtimeDocumentId"] = json!("doc:2"),
            |r| r["scriptAuthorization"]["expiresAtUnixMs"] = json!(u64::MAX),
            |r| r["scriptAuthorization"]["expiresAtUnixMs"] = Value::Null,
            |r| r["requestId"] = Value::Null,
            |r| r["clientId"] = json!(""),
        ];
        for edit in edits {
            let error = with(edit);
            assert!(error.starts_with("SCRIPT_AUTHORIZATION_INVALID"), "{error}");
        }
        assert!(authorize(&good, Some("doc:1"), at + 61_000).unwrap_err().starts_with("SCRIPT_AUTHORIZATION_EXPIRED"));
        assert!(authorize(&good, None, at).is_err(), "no Studio connection: no script");
    }

    #[test]
    fn an_authorized_script_draws_as_one_undo_step_and_a_failed_one_is_undone() {
        let link = Link::new(true);
        let mut app = app().with_studio(link.clone());
        crate::studio::publish(&app);
        let open = link.snapshot().document.runtime_id;
        let ctx = egui::Context::default();
        let count = |app: &mut CadApp| tool(app, "query-entities", json!({})).unwrap()["count"].as_u64().unwrap();

        let refused = call(&mut app, &ctx, &json!({ "name": "execute-script", "parameters": { "code": "(+ 1 2)" } })).unwrap_err();
        assert!(refused.starts_with("SCRIPT_AUTHORIZATION_REQUIRED"), "{refused}");

        let two_lines = r#"(command "line" (list 0 0) (list 5 0) "") (command "line" (list 0 1) (list 5 1) "") (* 6 7)"#;
        let done = call(&mut app, &ctx, &script_request(two_lines, &open)).unwrap();
        assert_eq!((done["value"].clone(), done["changed"].clone()), (json!("42"), json!(true)));
        assert_eq!(count(&mut app), 2);
        tool(&mut app, "undo", json!({})).unwrap();
        assert_eq!(count(&mut app), 0, "both lines were one undo step");

        let failing = r#"(command "line" (list 0 0) (list 5 0) "") (car 5)"#;
        let error = call(&mut app, &ctx, &script_request(failing, &open)).unwrap_err();
        assert!(error.starts_with("SCRIPT_FAILED") && error.contains("undone"), "{error}");
        assert_eq!(count(&mut app), 0, "the line it drew before failing is gone");

        // It may not leave the drawing it was sent to.
        let documents = app.session.docs.len();
        let _ = call(&mut app, &ctx, &script_request(r#"(command "new")"#, &open));
        assert_eq!(app.session.docs.len(), documents);
    }

    #[test]
    fn a_command_studio_runs_draws_and_undoes_like_one_the_person_typed() {
        let mut app = app();
        let drawn = tool(&mut app, "run-command", json!({ "command": "L", "params": { "points": [[0, 0], [10, 0]] } })).unwrap();
        assert_eq!(drawn["command"], "line", "the alias is resolved");
        let found = tool(&mut app, "query-entities", json!({ "type": "Line" })).unwrap();
        assert_eq!(found["count"], 1);
        tool(&mut app, "undo", json!({})).unwrap();
        assert_eq!(tool(&mut app, "query-entities", json!({})).unwrap()["count"], 0);
    }

    #[test]
    fn run_command_refuses_code_and_quitting_under_any_name() {
        let mut app = app();
        for command in ["lisp", "LISP", "script", "appload", "ui.quit", "studio.disconnect"] {
            let error = tool(&mut app, "run-command", json!({ "command": command, "params": { "code": "(+ 1 2)" } })).unwrap_err();
            assert!(error.starts_with("COMMAND_REFUSED"), "{command}: {error}");
        }
        let listed = tool(&mut app, "list-commands", json!({ "filter": "lisp", "limit": 1000 })).unwrap();
        assert!(listed["commands"].as_array().unwrap().iter().all(|c| c["id"] != "lisp"));
    }

    #[test]
    fn selecting_by_handle_is_what_get_selection_reads_back() {
        let mut app = app();
        tool(&mut app, "run-command", json!({ "command": "circle", "params": { "center": [0, 0], "radius": 5 } })).unwrap();
        let handle = tool(&mut app, "query-entities", json!({})).unwrap()["entities"][0]["handle"].clone();
        assert_eq!(tool(&mut app, "select", json!({ "ids": [handle] })).unwrap()["selectedCount"], 1);
        let read = tool(&mut app, "get-selection", json!({ "geometry": true })).unwrap();
        assert_eq!(read["selectedIds"], json!([handle]));
        assert_eq!(read["entities"][0]["type"], "Circle");
        assert_eq!(tool(&mut app, "select", json!({ "clear": true })).unwrap()["selectedCount"], 0);
    }

    #[test]
    fn hiding_layers_is_one_undo_step_and_reports_what_it_could_not_find() {
        let mut app = app();
        for name in ["Walls", "Doors"] {
            tool(&mut app, "run-command", json!({ "command": "layer.new", "params": { "name": name } })).unwrap();
        }
        let visible = |app: &mut CadApp, name: &str| {
            let listed = tool(app, "query-buckets", json!({})).unwrap();
            listed["buckets"].as_array().unwrap().iter().find(|b| b["name"] == name).map(|b| b["visible"] == true)
        };
        let states = json!({ "states": [{ "name": "Walls", "visible": false }, { "id": "layer:Doors", "visible": false }, { "name": "Nowhere", "visible": false }] });
        let done = tool(&mut app, "apply-layer-visibility", states).unwrap();
        assert_eq!(done["changed"], 2);
        assert_eq!(done["failed"][0]["name"], "Nowhere");
        assert_eq!(visible(&mut app, "Walls"), Some(false));
        assert_eq!(visible(&mut app, "Doors"), Some(false));
        tool(&mut app, "undo", json!({})).unwrap();
        assert_eq!(visible(&mut app, "Walls"), Some(true));
        assert_eq!(visible(&mut app, "Doors"), Some(true), "both layers came back with one undo");
        tool(&mut app, "apply-layer-visibility", json!({ "states": [{ "name": "Walls", "visible": false }] })).unwrap();
        tool(&mut app, "apply-layer-visibility", json!({ "showAll": true })).unwrap();
        assert_eq!(visible(&mut app, "Walls"), Some(true));
    }

    #[test]
    fn a_capture_is_a_png_with_the_view_that_restores_it() {
        let mut app = app();
        tool(&mut app, "run-command", json!({ "command": "rectang", "params": { "p1": [0, 0], "p2": [40, 20] } })).unwrap();
        let shot = tool(&mut app, "capture-view", json!({ "mode": "whole", "longEdgePx": 256 })).unwrap();
        assert!(shot["image"].as_str().unwrap().starts_with("iVBOR"), "base64 of a PNG");
        assert_eq!(shot["width"], 256);
        assert!(shot["height"].as_u64().unwrap() < 256);
        let before = shot["viewState"].clone();
        let framed = tool(&mut app, "frame-objects", json!({})).unwrap();
        assert_eq!(framed["previousViewState"], before);
        let back = tool(&mut app, "restore-view", json!({ "viewState": before })).unwrap();
        assert_eq!(back["viewState"]["center"], before["center"]);
        assert!(tool(&mut app, "capture-view", json!({ "mode": "sideways" })).is_err());
    }

    #[test]
    fn a_request_for_another_drawing_changes_nothing() {
        let link = Link::new(true);
        let mut app = app().with_studio(link.clone());
        crate::studio::publish(&app);
        let open = link.snapshot().document.runtime_id;
        let ctx = egui::Context::default();
        let draw = |expected: &str| {
            json!({ "name": "run-command", "expectedRuntimeDocumentId": expected,
                    "parameters": { "command": "line", "params": { "points": [[0, 0], [1, 1]] } } })
        };
        assert!(call(&mut app, &ctx, &draw("someone-else:1")).unwrap_err().starts_with("DOCUMENT_FENCE_MISMATCH"));
        assert_eq!(tool(&mut app, "query-entities", json!({})).unwrap()["count"], 0);
        call(&mut app, &ctx, &draw(&open)).unwrap();
        assert_eq!(tool(&mut app, "query-entities", json!({})).unwrap()["count"], 1);
    }

    #[test]
    fn a_change_waits_while_the_person_is_inside_a_command_and_reads_do_not() {
        let mut app = app();
        app.start("line");
        assert!(app.session.running.is_some());
        let error = tool(&mut app, "run-command", json!({ "command": "circle", "params": { "center": [0, 0], "radius": 1 } })).unwrap_err();
        assert!(error.starts_with("CAD_BUSY"), "{error}");
        assert!(tool(&mut app, "get-context", json!({})).is_ok());
        assert!(app.session.running.is_some(), "the person's command is still running");
    }

    #[test]
    fn the_context_names_the_open_drawing_and_is_empty_without_one() {
        let mut none = CadApp::new(Session::empty(), crate::Services::default());
        let empty = tool(&mut none, "get-context", json!({})).unwrap();
        assert_eq!(empty["document"], Value::Null);
        assert_eq!(empty["documents"], json!([]));
        let mut app = app();
        let context = tool(&mut app, "get-context", json!({})).unwrap();
        assert_eq!(context["documents"].as_array().unwrap().len(), 1);
        assert!(context["document"]["title"].is_string());
        assert!(context["document"].get("layers").is_none(), "layers come from query-buckets");
    }

    #[test]
    fn mesh_bundle_round_trips_lines_circles_and_external_meshes() {
        let mut src = app();
        tool(&mut src, "run-command", json!({ "command": "line", "params": { "points": [[0, 0], [100, 0]] } })).unwrap();
        tool(&mut src, "run-command", json!({ "command": "circle", "params": { "center": [50, 50], "radius": 25 } })).unwrap();
        let exported = tool(&mut src, "export-selected-mesh", json!({ "scope": "all" })).unwrap();
        assert_eq!(exported["exportedCount"], 2);
        let bundle = exported["bundle"].clone();
        assert_eq!(bundle["curves"].as_array().unwrap().len(), 2);

        let mut dst = app();
        let imported = tool(&mut dst, "import-mesh-bundle", json!({ "bundle": bundle })).unwrap();
        assert_eq!(imported["importedCount"], 2);
        assert_eq!(tool(&mut dst, "query-entities", json!({})).unwrap()["count"], 2);
        // Undo rolls back the entire import in one step.
        tool(&mut dst, "undo", json!({})).unwrap();
        assert_eq!(tool(&mut dst, "query-entities", json!({})).unwrap()["count"], 0);

        // External 3D mesh (e.g. from Rhino/Blender) also imports cleanly.
        let ext = json!({
            "schemaVersion": "3.1",
            "units": "mm",
            "meshes": [{ "id": "m1", "layerName": "RhinoWall", "vertices": [0,0,0, 10,0,0, 10,10,0], "indices": [0,1,2] }],
            "curves": []
        });
        let imp2 = tool(&mut dst, "import-mesh-bundle", json!({ "bundle": ext })).unwrap();
        assert_eq!(imp2["meshCount"], 1);
        assert_eq!(tool(&mut dst, "query-entities", json!({})).unwrap()["count"], 1);
    }

    // Studio's agent transfer compares these counts: a handle not in the drawing was counted as sent,
    // and the receiver said neither how many it was given nor what it left out.
    #[test]
    fn mesh_exchange_counts_what_was_found_given_and_left_out() {
        let mut src = app();
        tool(&mut src, "run-command", json!({ "command": "line", "params": { "points": [[0, 0], [100, 0]] } })).unwrap();
        let handle = tool(&mut src, "query-entities", json!({})).unwrap()["entities"][0]["handle"].clone();
        let exported = tool(&mut src, "export-selected-mesh", json!({ "scope": "selection-only", "ids": [handle, "FFFFFF"] })).unwrap();
        assert_eq!(exported["selectedCount"], 1, "the missing handle is not counted as selected");
        assert_eq!(exported["missingHandleCount"], 1);
        assert_eq!(exported["skipped"]["count"], 0);

        let mut dst = app();
        let triangles: Vec<u32> = (0..5_001u32).flat_map(|_| [0, 1, 2]).collect();
        let bundle = json!({
            "schemaVersion": "3.1", "units": "mm",
            "curves": [{ "points": [0,0,0, 10,0,0] }, { "points": [5,5,0] }],
            "meshes": [{ "vertices": [0,0,0, 10,0,0, 10,10,0], "indices": triangles }],
            "instances": [{ "definitionId": "d1" }],
        });
        let imported = tool(&mut dst, "import-mesh-bundle", json!({ "bundle": bundle })).unwrap();
        assert_eq!(imported["sourceCount"], 4);
        assert_eq!(imported["importedCount"], 2);
        assert_eq!(imported["skipped"]["count"], 2);
        assert_eq!(imported["skipped"]["byReason"], json!({ "unsupported": 1, "empty": 1 }));
        assert_eq!(imported["skipped"]["partialPieces"], 1);
        assert_eq!(imported["skipped"]["piecesByReason"], json!({ "limit": 1 }));
    }
}
