//! The PAULGA Studio link: what the desktop host's connection and the interface share, the
//! commands that open, close and switch it, and the tools Studio runs ([`tools`]).
//!
//! The host (`apps/cadcraft`) owns the socket. It reads [`Link::snapshot`] to learn whether a
//! connection is wanted and which drawing is open, writes back how the connection is doing, and
//! runs each tool Studio asks for on the UI thread through the control method `studio.tool`.

pub mod exchange;
pub mod panel;
pub mod tools;

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};

use crate::CadApp;

/// The name Studio knows this program by (`CadPlatform::PaulgaCad` on its side).
pub const PLATFORM: &str = "paulga-cad";

/// How the connection is doing, as the host last reported it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// The person switched the connection off.
    #[default]
    Off,
    /// No running Studio was found; the host keeps looking.
    Searching,
    /// A Studio was found and the sign-in is under way.
    Connecting,
    Connected,
    /// Studio refused the sign-in; the host waits for a new session or for Connect.
    Refused,
}

impl Phase {
    pub fn id(self) -> &'static str {
        match self {
            Phase::Off => "off",
            Phase::Searching => "searching",
            Phase::Connecting => "connecting",
            Phase::Connected => "connected",
            Phase::Refused => "refused",
        }
    }
    /// The state as the panel and the status command word it (English; translated when shown).
    pub fn label(self) -> &'static str {
        match self {
            Phase::Off => "Off",
            Phase::Searching => "Looking for PAULGA Studio",
            Phase::Connecting => "Connecting",
            Phase::Connected => "Connected",
            Phase::Refused => "PAULGA Studio refused the connection",
        }
    }
}

/// The open drawing as Studio is told about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub name: Option<String>,
    pub path: Option<String>,
    /// `<process>:<serial>`: names this run of the program and the drawing it shows. A request
    /// that carries another id was meant for a drawing that is no longer the open one.
    pub runtime_id: String,
    /// A command is waiting for input, so a tool that changes the drawing would be refused.
    pub busy: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// The person wants a connection (the panel's switch, `studio.connect`).
    pub wanted: bool,
    /// Bumped by Connect, so a host waiting after a refusal tries again at once.
    pub attempt: u64,
    pub phase: Phase,
    /// Why the connection is not up, in English.
    pub note: Option<String>,
    pub port: Option<u16>,
    pub studio_version: Option<String>,
    pub tools_run: u64,
    pub last_tool: Option<String>,
    pub document: Document,
}

/// Shared by the host's connection thread and the interface.
pub struct Link {
    state: Mutex<State>,
    /// `<pid>-<start, ms since 1970>`: one run of this program.
    process: String,
    /// What the open drawing was last time, and the serial it got.
    identity: Mutex<(Option<(usize, String, Option<String>)>, u32)>,
    /// The state the person was last told about ([`frame`]).
    told: Mutex<Phase>,
}

impl Link {
    pub fn new(wanted: bool) -> Arc<Self> {
        let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let link = Link {
            state: Mutex::new(State { wanted, ..State::default() }),
            process: format!("{}-{started}", std::process::id()),
            identity: Mutex::new((None, 1)),
            told: Mutex::new(Phase::Off),
        };
        link.edit(|s| s.document.runtime_id = format!("{}:1", link.process));
        Arc::new(link)
    }

    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn edit(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// The host says how the connection is doing.
    pub fn report(&self, phase: Phase, note: Option<String>) {
        self.edit(|s| {
            s.phase = phase;
            s.note = note;
            if phase != Phase::Connected {
                s.port = None;
            }
        });
    }

    /// The host is signed in to the Studio listening on `port`.
    pub fn connected(&self, port: u16, studio_version: Option<String>) {
        self.edit(|s| {
            s.phase = Phase::Connected;
            s.note = None;
            s.port = Some(port);
            s.studio_version = studio_version;
        });
    }

    /// The host ran a tool for Studio.
    pub fn ran(&self, tool: &str) {
        self.edit(|s| {
            s.tools_run = s.tools_run.saturating_add(1);
            s.last_tool = Some(tool.to_string());
        });
    }

    pub fn set_wanted(&self, wanted: bool) {
        self.edit(|s| {
            s.wanted = wanted;
            s.attempt = s.attempt.wrapping_add(1);
            // What the panel shows at once; the host reports what it finds within a moment.
            if !wanted {
                s.phase = Phase::Off;
                s.note = None;
                s.port = None;
            } else if s.phase == Phase::Off || s.phase == Phase::Refused {
                s.phase = Phase::Searching;
                s.note = None;
            }
        });
    }

    /// Record the drawing the app shows now. A different drawing gets the next serial.
    fn publish(&self, key: Option<(usize, String, Option<String>)>, busy: bool) {
        let serial = {
            let mut identity = self.identity.lock().unwrap_or_else(PoisonError::into_inner);
            if identity.0 != key {
                identity.0 = key.clone();
                identity.1 = identity.1.wrapping_add(1).max(1);
            }
            identity.1
        };
        let document = Document {
            name: key.as_ref().map(|(_, title, _)| title.clone()),
            path: key.and_then(|(_, _, path)| path),
            runtime_id: format!("{}:{serial}", self.process),
            busy,
        };
        self.edit(|s| {
            if s.document != document {
                s.document = document;
            }
        });
    }
}

/// Tell the link which drawing is open. Cheap; runs every frame and before each tool.
pub fn publish(app: &CadApp) {
    let Some(link) = &app.studio else { return };
    let key = app.session.docs.get(app.session.active).map(|d| (app.session.active, d.title.clone(), d.path.clone()));
    link.publish(key, app.session.running.is_some());
}

/// Once a frame: publish the open drawing, and say in the status line when the connection comes
/// up, drops or is refused. It changes without the person doing anything, wherever they look.
pub fn frame(app: &mut CadApp) {
    publish(app);
    let Some(link) = app.studio.clone() else { return };
    let now = link.snapshot().phase;
    let before = std::mem::replace(&mut *link.told.lock().unwrap_or_else(PoisonError::into_inner), now);
    let message = match (before, now) {
        (a, b) if a == b => return,
        (_, Phase::Connected) => crate::tl!("Connected to PAULGA Studio"),
        (_, Phase::Refused) => crate::tl!("PAULGA Studio refused the connection"),
        (Phase::Connected, _) => crate::tl!("Disconnected from PAULGA Studio"),
        _ => return,
    };
    app.set_status(message.to_string());
}

fn panel_json(operation: &str) -> Value {
    json!({ "operation": operation, "panel": crate::docking::Panel::Studio.id() })
}

/// Open the panel. The first time it docks with the palettes; later it returns where it was.
pub fn open_panel(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::command(app, false, &panel_json("open"))
}

/// Window ▸ PAULGA Studio: close the panel when it is shown, else open it.
pub fn toggle_panel(app: &mut CadApp) -> Result<Value, String> {
    if app.ui.docking.layout.contains(&crate::docking::Panel::Studio) {
        crate::docking::command(app, false, &panel_json("close"))
    } else {
        open_panel(app)
    }
}

/// `studio.status`: the connection as the panel shows it.
pub fn status(app: &CadApp) -> Value {
    let Some(link) = &app.studio else {
        return json!({ "available": false, "state": Phase::Off.id(), "message": UNAVAILABLE });
    };
    publish(app);
    let s = link.snapshot();
    json!({
        "available": true,
        "wanted": s.wanted,
        "state": s.phase.id(),
        "message": s.note.clone().unwrap_or_else(|| s.phase.label().to_string()),
        "port": s.port,
        "studioVersion": s.studio_version,
        "toolsRun": s.tools_run,
        "lastTool": s.last_tool,
        "document": { "name": s.document.name, "path": s.document.path, "runtimeDocumentId": s.document.runtime_id },
        "panelOpen": app.ui.docking.layout.contains(&crate::docking::Panel::Studio),
    })
}

/// What a build without the connection says (the web build, and desktops Studio does not run on).
pub const UNAVAILABLE: &str = "The PAULGA Studio connection is part of the Windows desktop app.";

/// The `studio.*` and panel commands; `None` for any other id.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        "ui.toggle.studio" => toggle_panel(app),
        "ui.panel.studio" => open_panel(app),
        "studio.status" => Ok(status(app)),
        "studio.connect" | "studio.disconnect" => {
            let Some(link) = app.studio.clone() else { return Some(Err(UNAVAILABLE.into())) };
            link.set_wanted(id == "studio.connect");
            Ok(status(app))
        }
        "studio.tool" => {
            // The automation interface runs the same tools Studio does, with the same checks.
            let ctx = egui::Context::default();
            tools::call(app, &ctx, params)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    fn app() -> CadApp {
        CadApp::new(Session::empty(), crate::Services::default())
    }

    #[test]
    fn a_build_without_the_connection_says_so_instead_of_pretending() {
        let mut app = app();
        assert_eq!(app.run("studio.status", json!({})).unwrap()["available"], false);
        assert!(app.run("studio.connect", json!({})).is_err());
    }

    #[test]
    fn connect_and_disconnect_reach_the_host_and_wake_it() {
        let link = Link::new(true);
        let mut app = app().with_studio(link.clone());
        let before = link.snapshot().attempt;
        let off = app.run("studio.disconnect", json!({})).unwrap();
        assert_eq!(off["wanted"], false);
        assert!(!link.snapshot().wanted);
        app.run("studio.connect", json!({})).unwrap();
        let s = link.snapshot();
        assert!(s.wanted);
        assert_ne!(s.attempt, before);
    }

    #[test]
    fn another_drawing_gets_another_runtime_id_and_the_same_one_keeps_it() {
        let link = Link::new(true);
        let mut app = app().with_studio(link.clone());
        app.run("new", json!({})).unwrap();
        publish(&app);
        let first = link.snapshot().document.runtime_id;
        publish(&app);
        assert_eq!(link.snapshot().document.runtime_id, first);
        app.run("new", json!({})).unwrap();
        publish(&app);
        let second = link.snapshot().document;
        assert_ne!(second.runtime_id, first);
        assert!(second.name.is_some());
    }

    #[test]
    fn the_person_is_told_when_the_connection_comes_up_and_when_it_drops() {
        let link = Link::new(true);
        let mut app = app().with_studio(link.clone());
        frame(&mut app);
        assert!(app.status.is_none(), "nothing to say while it only looks for Studio");
        link.connected(38360, None);
        frame(&mut app);
        let up = app.status.take().map(|(text, _)| text).unwrap();
        link.report(Phase::Searching, None);
        frame(&mut app);
        let down = app.status.take().map(|(text, _)| text).unwrap();
        assert_ne!(up, down);
        frame(&mut app);
        assert!(app.status.is_none(), "said once");
    }

    #[test]
    fn the_panel_opens_and_closes_from_its_window_menu_command() {
        let mut app = app();
        app.run("new", json!({})).unwrap();
        let shown = |app: &CadApp| app.ui.docking.layout.contains(&crate::docking::Panel::Studio);
        assert!(!shown(&app));
        app.run("ui.toggle.studio", json!({})).unwrap();
        assert!(shown(&app));
        app.run("ui.toggle.studio", json!({})).unwrap();
        assert!(!shown(&app));
        app.run("ui.panel.studio", json!({})).unwrap();
        app.run("ui.panel.studio", json!({})).unwrap();
        assert!(shown(&app), "opening twice keeps it open");
    }
}
