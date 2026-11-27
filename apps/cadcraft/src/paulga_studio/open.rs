//! The PAULGA Studio button: bring the running Studio's window forward, or start the installed one.
//!
//! The running Studio is the one its connection file names ([`bridge::desktop_pids`]), whether or
//! not this program is connected to it. The installed one is the folder its installer recorded in
//! Windows' list of installed programs; only the executable of that name directly inside that
//! folder is started, with no arguments.

use std::path::{Path, PathBuf};
use std::process::Command;

use winsafe::co;

use super::bridge;

/// The installer's entry in Windows' list of installed programs, under HKCU for a one-person
/// install and HKLM for an all-users one.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PAULGA Studio";
const DISPLAY_NAME: &str = "PAULGA Studio";
const EXECUTABLE: &str = "paulga-studio.exe";

/// A top-level window, as far as choosing Studio's goes.
#[derive(Clone, Copy)]
struct Window {
    pid: u32,
    visible: bool,
    /// It has an owner: a dialog or a tool window of another window.
    owned: bool,
    titled: bool,
}

/// Whether `window` is a main window of one of the Studio processes `pids`: shown, with a title
/// and no owner. A Studio kept in the notification area has none.
fn is_main_window(window: Window, pids: &[u32]) -> bool {
    window.visible && window.titled && !window.owned && pids.contains(&window.pid)
}

/// Whether the program at `image` (a process's executable path) is Studio's.
fn is_studio_image(image: &str) -> bool {
    Path::new(image).file_name().is_some_and(|name| name.eq_ignore_ascii_case(EXECUTABLE))
}

/// Studio's executable in the folder `location` names (an `InstallLocation` as the installer
/// wrote it, quoted or not): an absolute folder that exists, and the file of Studio's name
/// directly inside it. A name that leads out of the folder (a link) is not taken.
fn executable_in(location: &str) -> Option<PathBuf> {
    let folder = PathBuf::from(location.trim().trim_matches('"'));
    if !folder.is_absolute() {
        return None;
    }
    let folder = std::fs::canonicalize(folder).ok()?;
    let file = std::fs::canonicalize(folder.join(EXECUTABLE)).ok()?;
    (file.is_file() && file.parent() == Some(folder.as_path())).then_some(file)
}

/// The text value `name` of Studio's entry under `root`, when the entry is Studio's.
fn recorded(root: &winsafe::HKEY, name: &str) -> Option<String> {
    let key = root.RegOpenKeyEx(Some(UNINSTALL_KEY), co::REG_OPTION::default(), co::KEY::QUERY_VALUE).ok()?;
    let text = |name: &str| match key.RegQueryValueEx(Some(name)) {
        Ok(winsafe::RegistryValue::Sz(text) | winsafe::RegistryValue::ExpandSz(text)) => Some(text),
        _ => None,
    };
    (text("DisplayName").as_deref() == Some(DISPLAY_NAME)).then(|| text(name)).flatten()
}

/// The installed Studio's executable: the person's own install, then the computer's.
fn installed() -> Option<PathBuf> {
    [winsafe::HKEY::CURRENT_USER, winsafe::HKEY::LOCAL_MACHINE].iter().find_map(|root| recorded(root, "InstallLocation").as_deref().and_then(executable_in))
}

/// Whether the process `pid` is a running Studio (a connection file can outlive its Studio, and
/// its process id be given to another program).
fn is_studio(pid: u32) -> bool {
    winsafe::HPROCESS::OpenProcess(co::PROCESS::QUERY_LIMITED_INFORMATION, false, pid)
        .ok()
        .and_then(|process| process.QueryFullProcessImageName(co::PROCESS_NAME::WIN32).ok())
        .is_some_and(|image| is_studio_image(&image))
}

/// Bring the main window of a running Studio forward, restored if it is minimised. False when no
/// Studio runs or it shows no window.
fn raise_running() -> bool {
    let Some(root) = std::env::var_os("APPDATA").map(PathBuf::from) else { return false };
    let pids: Vec<u32> = bridge::desktop_pids(&root).into_iter().filter(|pid| is_studio(*pid)).collect();
    if pids.is_empty() {
        return false;
    }
    let mut raised = false;
    // Top-most first; the walk stops at Studio's window (which the function reports as an error).
    let _ = winsafe::EnumWindows(|hwnd| {
        let window = Window {
            pid: hwnd.GetWindowThreadProcessId().1,
            visible: hwnd.IsWindowVisible(),
            owned: hwnd.GetWindow(co::GW::OWNER).is_ok(),
            titled: hwnd.GetWindowTextLength().is_ok_and(|length| length > 0),
        };
        if !is_main_window(window, &pids) {
            return true;
        }
        if hwnd.IsIconic() {
            hwnd.ShowWindow(co::SW::RESTORE);
        }
        // The person has just clicked in this program's window, so it may hand the foreground on.
        raised = hwnd.SetForegroundWindow();
        false
    });
    raised
}

/// The button's action. `Ok(true)`: Studio's window came forward, or the installed Studio was
/// started. `Ok(false)`: Studio is not installed on this computer.
pub(crate) fn open_studio() -> Result<bool, String> {
    if raise_running() {
        return Ok(true);
    }
    let Some(executable) = installed() else { return Ok(false) };
    let mut start = Command::new(&executable);
    if let Some(folder) = executable.parent() {
        start.current_dir(folder);
    }
    let studio = start.spawn().map_err(|error| format!("PAULGA Studio could not be started: {error}"))?;
    // Studio's window may come to the front when it opens (a running Studio shows its own).
    let _ = winsafe::AllowSetForegroundWindow(Some(studio.id()));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_is_found_only_as_its_own_file_directly_in_the_recorded_folder() {
        let root = std::env::temp_dir().join(format!("paulga-studio-open-{}-{:?}", std::process::id(), std::thread::current().id()));
        let (installed, elsewhere) = (root.join("PAULGA Studio"), root.join("Other"));
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(elsewhere.join(EXECUTABLE)).unwrap();
        std::fs::write(installed.join(EXECUTABLE), b"x").unwrap();
        let folder = installed.to_string_lossy().into_owned();
        // The installer writes the folder in quotes.
        for location in [folder.clone(), format!("\"{folder}\""), format!("  \"{folder}\" ")] {
            let found = executable_in(&location).unwrap();
            assert_eq!(found.file_name().unwrap(), EXECUTABLE, "{location}");
            assert_eq!(found.parent(), std::fs::canonicalize(&installed).ok().as_deref());
        }
        assert_eq!(executable_in(&root.join("gone").to_string_lossy()), None, "no such folder");
        assert_eq!(executable_in(&root.to_string_lossy()), None, "Studio is not in it");
        assert_eq!(executable_in(&elsewhere.to_string_lossy()), None, "a folder of that name is not the program");
        assert_eq!(executable_in("PAULGA Studio"), None, "not an absolute folder");
        assert_eq!(executable_in(""), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_window_brought_forward_is_a_shown_main_window_of_a_running_studio() {
        let main = Window { pid: 7, visible: true, owned: false, titled: true };
        assert!(is_main_window(main, &[3, 7]));
        assert!(!is_main_window(main, &[3]), "another program's window");
        assert!(!is_main_window(main, &[]));
        assert!(!is_main_window(Window { visible: false, ..main }, &[7]), "a Studio kept in the notification area");
        assert!(!is_main_window(Window { owned: true, ..main }, &[7]), "a dialog of it");
        assert!(!is_main_window(Window { titled: false, ..main }, &[7]), "a helper window");
    }

    #[test]
    fn a_process_is_studio_by_the_name_of_its_program() {
        assert!(is_studio_image(r"C:\Users\me\AppData\Local\PAULGA Studio\paulga-studio.exe"));
        assert!(is_studio_image(r"D:\dev\target\debug\PAULGA-Studio.EXE"));
        assert!(!is_studio_image(r"C:\Windows\explorer.exe"));
        assert!(!is_studio_image(r"C:\tools\not-paulga-studio.exe"));
        assert!(!is_studio_image(""));
    }
}
