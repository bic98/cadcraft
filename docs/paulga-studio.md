# PAULGA Studio connection

PAULGA CAD connects to the PAULGA Studio desktop app on the same computer, so Studio can read the
open drawing, capture its view and run PAULGA CAD commands in it. The connection is built into
the Windows desktop app: there is nothing to install.

## Using it

- **Window ▸ PAULGA Studio** opens and closes the panel. It docks with the palettes, floats, and
  remembers its place like the others (panel id `studio`).
- The panel shows the state (Off, Looking for PAULGA Studio, Connecting, Connected, refused), the
  drawing Studio sees, Studio's version and port, and how many tools Studio has run.
- **Connect / Disconnect** switches the connection. The choice is kept across restarts; it is on
  by default. **Try Again** looks for Studio at once.
- The status line says when the connection comes up, drops or is refused, and names each tool
  that changed the drawing.

The same through the automation interface (`engine.execute`):

| Command | Result |
|---|---|
| `ui.toggle.studio` / `ui.panel.studio` | Close-or-open / open the panel. |
| `studio.connect` / `studio.disconnect` | Switch the connection; replies as `studio.status`. |
| `studio.status` | `{available, wanted, state, message, port, studioVersion, toolsRun, lastTool, document, panelOpen}`. |
| `studio.tool` `{name, parameters?, expectedRuntimeDocumentId?}` | Run a tool exactly as Studio's connection does. Also a control method. |

On the web build and on desktops Studio does not run on, `studio.status` replies
`available: false` and the others fail with that reason; the panel says the same.

## How it connects

1. Studio writes `%APPDATA%\@paulga\<app>\bridge.json`: a session token, its process id and, per
   program, whether it listens and on which loopback port (`status["paulga-cad"]`, ports
   38360–38369).
2. PAULGA CAD reads the freshest file whose Studio has not marked itself as exited and connects
   only to the port recorded for it. Other ports are never probed.
3. Before the token is sent, it checks that the process on that port is the Studio the file names
   (`desktopPid`). A stale file cannot hand the token to another program.
4. It signs in (`x-paulga-session` header, then `auth`), and sends `cad:status` with the open
   drawing whenever that changes.

A dropped connection is retried after 1, 2, 4, 8, 16 s. A Studio that said it is exiting is not
tried again; a refused token is not sent again until Studio restarts or Connect is pressed.

Code: `apps/cadcraft/src/paulga_studio/` (the socket), `crates/ui-egui/src/studio.rs` (state,
commands), `studio/tools.rs` (tools), `studio/panel.rs` (panel).

## Tools

`get-capabilities` lists exactly these; Studio's allowlist for `paulga-cad` names the same 14.
Coordinates are drawing units.

| Tool | Access | What it does |
|---|---|---|
| `get-capabilities` | read | This list, with parameter schemas. |
| `get-context` | read | Open drawings and the active one: name, path, space, current layer, extents, view, counts. |
| `get-selection` | read | Selected handles; `geometry: true` adds layers and geometry. |
| `query-entities` | read | Objects by `type`, `layer`, `selected`, `window`; `limit` (200, at most 5000), `offset`. |
| `query-buckets` | read | Layers as buckets (`layer:<name>`, colour, `visible`); `counts: true` adds object counts. |
| `list-commands` | read | The command catalog for `run-command`: id, aliases, JSON parameters. |
| `capture-view` | read | PNG of the `current` view or the `whole` drawing (`longEdgePx` 256–8192, `background`), with its `viewState`. |
| `restore-view` | change | Move the view to a `viewState`. |
| `frame-objects` | change | Zoom to `ids`, or to the whole drawing; returns the view it left. |
| `select` | change | Select `ids`, `add` to the selection, or `clear` it. |
| `apply-layer-visibility` | change | `states: [{name, visible}]` or `showAll`; one undo step. |
| `run-command` | change | One registered command with JSON `params`. Never opens a dialog. |
| `undo` | change | Undo `count` changes (default 1). |
| `execute-script` | script | Run AutoLISP `code` (at most 16000 characters; `inputs` answer its `get…` calls). One undo step. |

Rules every tool follows:

- **One drawing.** A request that carries `expectedRuntimeDocumentId` runs only if that drawing is
  still the open one (`DOCUMENT_FENCE_MISMATCH` otherwise).
- **The person first.** A change is refused while a command waits for input (`CAD_BUSY`); reads
  still answer.
- **Code only through `execute-script`.** `run-command` refuses `SCRIPT`, `LISP`, `APPLOAD` and
  quitting (`COMMAND_REFUSED`), under any alias.
- **Undo.** Each change is an ordinary undo step.
- Results over 50 MB are answered as a failure (`RESULT_TOO_LARGE`), not dropped.

## Scripts

`execute-script` runs AutoLISP that Studio sent. It runs only when all of this holds:

- Script execution is switched on for PAULGA CAD in Studio (Profile > App info).
- The request carries Studio's `scriptAuthorization`, beside the parameters: it names this
  request, this connection and the drawing that is open, and expires within five minutes.
  PAULGA CAD takes the request id and the connection id from the connection itself, so an
  authorization written into the tool's parameters grants nothing
  (`SCRIPT_AUTHORIZATION_REQUIRED`, `_INVALID`, `_EXPIRED`).
- No command is waiting for the person's input (`CAD_BUSY`).

While it runs:

- **Files.** It and every command it starts reach files only in
  `%APPDATA%@paulgapaulga-cadscripts` (the engine's automation roots,
  `cadcraft_engine::file_access`). `(open …)` of a file elsewhere returns nil.
- **One drawing.** `NEW`, `OPEN`, `CLOSE`, `CLOSEALL`, `RECOVER` and switching drawings are
  refused.
- **Nothing left behind.** Functions, `c:` commands and variables it defines are gone when it
  ends, so nothing of it runs later with the person's own access. The person's own definitions
  are as they were.
- **Undo.** The whole script is one undo step. A script that ends in an error is undone
  (`SCRIPT_FAILED`, with its last output).
- **No waiting.** A `get…` call with no `inputs` left gets nil; `pause` does not wait.

The same run without Studio, for automation that already has the roots in force:
`lisp {code, script: true}`.

## Not yet

- Studio's layer canvas and render-source menu have no PAULGA CAD adapter yet; the tools above
  are reached from Studio's agent and MCP.
- `run-command` can open and save drawings by path. Files are not limited to Studio's folders
  (a script's are).
- A script runs on the interface thread to its end; a long one holds the window until the
  interpreter's step limit stops it.
- The Studio check is by process id; Studio's code signature is not verified.
- macOS and Linux: Studio writes its connection file only on Windows.
