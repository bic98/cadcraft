//! AutoLISP commands: APPLOAD loads a `.lsp` file; `lisp` evaluates LISP for programmatic
//! callers (scripts, the control channel, MCP, the CLI).

use serde_json::{Value, json};

use super::{bad, str_param};
use crate::lisp::value::{Value as Lv, to_text};
use crate::{Accept, CommandSpec, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("appload", "Load Application...", run_appload)
            .menu(&["Tools", "Load Application..."])
            .alias(&["ap"])
            .params("{path: .lsp file} | {name, data: base64 | text} → {value?, commands: [commands the file defined], error?}")
            .noundo()
            .interactive(|_| Ok(Box::new(ApploadM))),
        CommandSpec::new("lisp", "Evaluate LISP", run_lisp)
            .params(
                "{code: LISP source, inputs?: [answers to get… calls: numbers, strings, [x,y] points], script?: bool (a script another program sent: only inside the automation roots, it stays in its drawing and leaves no definitions)} → {value (printed as prin1), output: lines printed, error?}",
            )
            .noundo(),
    ]
}

/// Whether `path` names an AutoLISP routine: APPLOAD loads `.lsp` files only, in any case.
fn is_lisp_path(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("lsp"))
}

/// The text of a LISP file from `{path}` or `{name, data | text}`, and its name.
fn source(p: &Value) -> Result<(String, String)> {
    if let Some(text) = str_param(p, "text") {
        return Ok((str_param(p, "name").unwrap_or("untitled.lsp").to_string(), text.to_string()));
    }
    if let Some(data) = str_param(p, "data") {
        let bytes = super::file::base64_decode(data).ok_or_else(|| bad("appload", "invalid base64"))?;
        return Ok((str_param(p, "name").unwrap_or("untitled.lsp").to_string(), String::from_utf8_lossy(&bytes).into_owned()));
    }
    // The path is read as it is given (a JSON caller may name a file or folder with spaces);
    // only a path of blanks alone is refused.
    let path = str_param(p, "path")
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| bad("appload", "`path`, or `name` with `data` (base64) or `text`, is required"))?;
    if !is_lisp_path(path) {
        return Err(bad("appload", format!("{path}: not an AutoLISP file (APPLOAD loads .lsp files)")));
    }
    let text = crate::lisp::builtins::read_text_file(path).ok_or_else(|| bad("appload", format!("{path}: can't read the file")))?;
    Ok((path.to_string(), text))
}

/// The result of a blocking LISP run as JSON, with the lines it printed.
fn report(s: &Session, from_log: usize, r: Option<std::result::Result<Lv, String>>) -> Value {
    let output: Vec<String> = s.log.get(from_log..).map(|l| l.to_vec()).unwrap_or_default();
    match r {
        Some(Ok(v)) => json!({ "value": to_text(&v, true), "output": output }),
        Some(Err(e)) => json!({ "error": e, "output": output }),
        None => json!({ "output": output }),
    }
}

fn run_appload(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, text) = source(p)?;
    let before = s.lisp_commands();
    let from = s.log.len();
    let r = s.lisp_load(&name, &text, true)?;
    let mut out = report(s, from, r);
    // The commands the file defined, so a caller can run them next (a failed file may still
    // have defined some before the error).
    out["commands"] = json!(s.lisp_commands().into_iter().filter(|c| !before.contains(c)).collect::<Vec<_>>());
    Ok(out)
}

/// A JSON input as a LISP value: numbers, strings, `[x, y(, z)]` points, null for Enter.
fn input_value(v: &Value) -> Lv {
    match v {
        Value::Array(a) if a.len() >= 2 && a.iter().all(Value::is_number) => Lv::point(&a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()),
        other => crate::lisp::drawing_from_json(other),
    }
}

fn run_lisp(s: &mut Session, p: &Value) -> Result<Value> {
    let code = str_param(p, "code").ok_or_else(|| bad("lisp", "`code` is required"))?;
    let inputs = p.get("inputs").and_then(Value::as_array).map(|a| a.iter().map(input_value).collect()).unwrap_or_default();
    let from = s.log.len();
    let script = p.get("script").and_then(Value::as_bool).unwrap_or(false);
    let run = if script { s.lisp_eval_script(code, inputs) } else { s.lisp_eval(code, false, true, inputs) };
    let r = run.map_err(|e| match e {
        EngineError::Other(m) => bad("lisp", m),
        e => e,
    })?;
    Ok(report(s, from, r))
}

/// APPLOAD typed or chosen from the menu: asks for the file by name.
struct ApploadM;

impl Interactive for ApploadM {
    fn name(&self) -> &'static str {
        "APPLOAD"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Enter name of LISP file to load", Accept::TEXT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel | Input::Enter) {
            return Ok(Step::Cancel);
        }
        let Input::Text(t) = i else { return Ok(Step::Continue) };
        let path = t.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return Ok(Step::Cancel);
        }
        match source(&json!({ "path": path })) {
            Ok((name, text)) => {
                // The routine may wait for the user: it carries on after this command ends.
                if let Err(e) = s.lisp_load(&name, &text, false) {
                    s.echo(e.to_string());
                }
                Ok(Step::Done)
            }
            Err(EngineError::BadParams { msg, .. }) => {
                s.echo(msg);
                Ok(Step::Done)
            }
            Err(e) => Err(e),
        }
    }
}

/// Script mode: a script another program sent, inside the automation roots.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod script_tests {
    use crate::file_access::{AutomationRoots, confine};
    use crate::cmd::file::{EntityCodec, set_entity_codec, set_object_records};
    use crate::temp_dir::TempDir;
    use serde_json::{Value, json};

    fn session() -> crate::Session {
        set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
        set_object_records(cadcraft_io::object_records);
        crate::Session::new()
    }

    /// Run `code` as a script confined to `dir`.
    fn script(s: &mut crate::Session, dir: &TempDir, code: &str) -> crate::Result<Value> {
        let roots = AutomationRoots::new(Some(dir), Some(dir)).unwrap();
        confine(roots.as_ref(), || s.execute("lisp", &json!({ "code": code, "script": true })))
    }

    #[test]
    fn a_script_draws_but_runs_only_inside_the_roots() {
        let (mut s, dir) = (session(), TempDir::new("script-roots"));
        let unconfined = s.execute("lisp", &json!({ "code": "(+ 1 2)", "script": true }));
        assert!(unconfined.is_err(), "no roots in force: refused, not run unconfined");
        let drawn = script(&mut s, &dir, r#"(command "line" (list 0 0) (list 5 5) "") (+ 1 2)"#).unwrap();
        assert_eq!(drawn["value"], "3");
        assert_eq!(s.doc().unwrap().entity_count(), 1);
    }

    #[test]
    fn a_script_reaches_no_file_outside_the_roots() {
        let (mut s, dir) = (session(), TempDir::new("script-files"));
        let outside = TempDir::new("script-outside");
        let secret = outside.file("secret.txt", "secret").replace('\\', "/");
        let read = script(&mut s, &dir, &format!(r#"(open "{secret}" "r")"#)).unwrap();
        assert_eq!(read["value"], "nil", "a file outside the roots does not open");
        let target = outside.path_str("written.txt").replace('\\', "/");
        script(&mut s, &dir, &format!(r#"(setq f (open "{target}" "w")) (if f (progn (write-line "x" f) (close f)))"#)).unwrap();
        assert!(!outside.join("written.txt").exists());
        let inside = dir.path_str("note.txt").replace('\\', "/");
        script(&mut s, &dir, &format!(r#"(setq f (open "{inside}" "w")) (write-line "kept" f) (close f)"#)).unwrap();
        assert!(dir.join("note.txt").exists(), "its own folder is where it may write");
    }

    #[test]
    fn a_script_stays_in_its_drawing() {
        let (mut s, dir) = (session(), TempDir::new("script-drawing"));
        let before = s.docs.len();
        for command in ["new", "close", "closeall"] {
            let run = script(&mut s, &dir, &format!(r#"(command "{command}")"#)).unwrap();
            assert_eq!(s.docs.len(), before, "{command}: {run}");
        }
        // The same command typed by the person afterwards still works.
        s.execute("new", &json!({})).unwrap();
        assert_eq!(s.docs.len(), before + 1);
    }

    #[test]
    fn what_a_script_defines_is_gone_when_it_ends() {
        let (mut s, dir) = (session(), TempDir::new("script-defs"));
        s.execute("lisp", &json!({ "code": "(setq mine 7)" })).unwrap();
        script(&mut s, &dir, r#"(defun c:trap () (open "x" "w")) (setq mine 8) (setq theirs 1)"#).unwrap();
        let after = s.execute("lisp", &json!({ "code": "(list mine theirs c:trap)" })).unwrap();
        assert_eq!(after["value"], "(7 nil nil)", "the person's variable is back; the script's command and variable are gone");
    }
}
