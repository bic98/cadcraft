//! PAULGA Studio's selection id files: `software-snapshots/<uuid>.ids`, format v1.
//!
//! A selection card in Studio keeps its object ids in one file Studio owns, so a card is not limited by
//! how many ids fit in one tool call. `get-selection` with `idsFilePath` writes the selection into the
//! new file Studio names; a tool given `handlesFile` reads the ids from it.
//!
//! ```text
//! #paulga-ids v1 <platform>
//! <id>\t<layer>\t<colorHex>
//! ```
//!
//! UTF-8, LF line ends. In a layer name `\`, TAB, CR and LF are written `\\`, `\t`, `\r`, `\n`.
//! Only a file named `<uuid>.ids` in a folder named `software-snapshots` under `%APPDATA%` is written
//! or read, by its real path: nothing else on the disk is reachable through these two parameters.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

const HEADER: &str = "#paulga-ids v1 ";
/// A sanity bound on one file, not a product limit (Studio's own reader has the same).
pub const MAX_IDS: usize = 5_000_000;
const MAX_LINE_BYTES: usize = 64 * 1024;

/// `<uuid>.ids` in a folder named `software-snapshots`.
fn named(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let Some(stem) = name.strip_suffix(".ids") else { return false };
    let uuid = stem.len() == 36
        && stem.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() });
    uuid && path.parent().and_then(Path::file_name).and_then(|n| n.to_str()) == Some("software-snapshots")
}

/// `dir` (a real path) lies under `%APPDATA%`.
fn under_app_data(dir: &Path) -> bool {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .and_then(|root| root.canonicalize().ok())
        .is_some_and(|root| dir.starts_with(root))
}

/// The new file `get-selection` is asked to write: it must not exist yet, in an existing folder.
pub fn new_file(raw: &str) -> Result<PathBuf, String> {
    let refused = || "idsFilePath must be a new software-snapshots/<uuid>.ids path".to_string();
    let path = PathBuf::from(raw);
    if !path.is_absolute() || !named(&path) {
        return Err(refused());
    }
    let dir = path.parent().and_then(|dir| dir.canonicalize().ok()).ok_or_else(refused)?;
    let real = dir.join(path.file_name().ok_or_else(refused)?);
    if !named(&real) || !under_app_data(&dir) || real.symlink_metadata().is_ok() {
        return Err(refused());
    }
    Ok(real)
}

/// The file a tool is asked to read its ids from: an existing plain file, by its real path.
pub fn existing_file(raw: &str) -> Result<PathBuf, String> {
    let refused = || "handlesFile must be a software-snapshots/<uuid>.ids file".to_string();
    let path = PathBuf::from(raw);
    if !path.is_absolute() || !named(&path) {
        return Err(refused());
    }
    let plain = path.symlink_metadata().map(|meta| meta.file_type().is_file()).unwrap_or(false);
    let real = path.canonicalize().map_err(|_| refused())?;
    let dir = real.parent().ok_or_else(refused)?;
    if !plain || !named(&real) || !under_app_data(dir) {
        return Err(refused());
    }
    Ok(real)
}

fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

/// Writes `rows` (`id`, `layer`, `colour` as `#rrggbb` or empty) for `platform` into the new file at
/// `path`. A row whose id is empty or holds a tab or a line end is not an id of this program: refused.
pub fn write(path: &Path, platform: &str, rows: &[(String, String, String)]) -> Result<(), String> {
    if rows.len() > MAX_IDS {
        return Err(format!("CAD_SIZE_LIMIT: more than {MAX_IDS} objects are selected"));
    }
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("CAD_FILE_IO: {e}"))?;
    let mut out = BufWriter::new(file);
    let io = |e: std::io::Error| format!("CAD_FILE_IO: {e}");
    writeln!(out, "{HEADER}{platform}").map_err(io)?;
    for (id, layer, colour) in rows {
        if id.is_empty() || id.contains(['\t', '\r', '\n']) {
            return Err("CAD_FILE_IO: an object id cannot be written to the selection file".into());
        }
        writeln!(out, "{id}\t{}\t{colour}", escaped(layer)).map_err(io)?;
    }
    out.flush().map_err(io)
}

/// The ids (first column) of the file at `path`, in its order.
pub fn read_ids(path: &Path) -> Result<Vec<String>, String> {
    let invalid = |why: &str| format!("CAD_SELECTION_IDS_INVALID: {why}");
    let file = std::fs::File::open(path).map_err(|e| format!("CAD_FILE_IO: {e}"))?;
    let mut lines = BufReader::new(file).lines();
    let header = lines.next().ok_or_else(|| invalid("empty file"))?.map_err(|e| format!("CAD_FILE_IO: {e}"))?;
    if !header.starts_with(HEADER) {
        return Err(invalid("bad header"));
    }
    let mut ids = Vec::new();
    for line in lines {
        let line = line.map_err(|e| format!("CAD_FILE_IO: {e}"))?;
        if line.len() > MAX_LINE_BYTES {
            return Err(invalid("a line is too long"));
        }
        let id = line.split('\t').next().unwrap_or("");
        if id.is_empty() {
            return Err(invalid("a line without an id"));
        }
        if ids.len() >= MAX_IDS {
            return Err(invalid("too many ids"));
        }
        ids.push(id.to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshots() -> Option<PathBuf> {
        let root = PathBuf::from(std::env::var_os("APPDATA")?).join("paulga-ids-file-test").join("software-snapshots");
        std::fs::create_dir_all(&root).ok()?;
        Some(root)
    }

    #[test]
    fn writes_the_file_studio_reads_and_reads_it_back() {
        let Some(dir) = snapshots() else { return };
        let raw = dir.join("11111111-2222-4333-8444-555555555555.ids");
        let _ = std::fs::remove_file(&raw);
        let path = new_file(raw.to_str().unwrap()).unwrap();
        let rows = vec![("1A".to_string(), "WALL\tA\\B".to_string(), "#ff0000".to_string()), ("2B".to_string(), String::new(), String::new())];
        write(&path, "paulga-cad", &rows).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "#paulga-ids v1 paulga-cad\n1A\tWALL\\tA\\\\B\t#ff0000\n2B\t\t\n");
        // Written once: the same path is not a new file any more.
        assert!(new_file(raw.to_str().unwrap()).is_err());
        let read = existing_file(raw.to_str().unwrap()).unwrap();
        assert_eq!(read_ids(&read).unwrap(), ["1A", "2B"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn takes_no_other_path() {
        let Some(dir) = snapshots() else { return };
        let outside = dir.parent().unwrap().join("11111111-2222-4333-8444-555555555555.ids");
        for raw in [
            "relative/software-snapshots/11111111-2222-4333-8444-555555555555.ids".to_string(),
            dir.join("notes.ids").to_string_lossy().into_owned(),
            dir.join("11111111-2222-4333-8444-555555555555.txt").to_string_lossy().into_owned(),
            outside.to_string_lossy().into_owned(),
            dir.join("..").join("software-snapshots").join("..").join("x.ids").to_string_lossy().into_owned(),
        ] {
            assert!(new_file(&raw).is_err(), "{raw}");
            assert!(existing_file(&raw).is_err(), "{raw}");
        }
        // Outside %APPDATA%: a folder of the right name elsewhere is not Studio's.
        let elsewhere = std::env::temp_dir().join("paulga-ids-elsewhere").join("software-snapshots");
        if std::env::temp_dir().canonicalize().is_ok_and(|temp| !under_app_data(&temp)) && std::fs::create_dir_all(&elsewhere).is_ok() {
            assert!(new_file(elsewhere.join("11111111-2222-4333-8444-555555555555.ids").to_str().unwrap()).is_err());
        }
    }

    #[test]
    fn refuses_a_file_that_is_not_an_ids_file() {
        let Some(dir) = snapshots() else { return };
        let path = dir.join("99999999-2222-4333-8444-555555555555.ids");
        std::fs::write(&path, "not ids\n1A\n").unwrap();
        assert!(read_ids(&path).unwrap_err().contains("bad header"));
        std::fs::write(&path, "#paulga-ids v1 paulga-cad\n\tWALL\t\n").unwrap();
        assert!(read_ids(&path).unwrap_err().contains("without an id"));
        let _ = std::fs::remove_file(&path);
    }
}
