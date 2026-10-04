//! `cellar --uninstall`: remove the program and everything Cellar stored
//! for itself (see [`super::app_dirs`]). Workbooks are never touched.

use std::io;
use std::path::{Path, PathBuf};

use super::app_dirs;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Settings, caches, state.
    Data,
    /// A link to the program, e.g. `/usr/local/bin/cellar`.
    Link,
    /// The program itself (on macOS the whole `Cellar.app`).
    Program,
}

#[derive(Debug)]
pub struct Item {
    pub path: PathBuf,
    pub what: &'static str,
    pub kind: Kind,
}

/// What an uninstall removes, in order: stored data, links, the program.
/// Only paths that exist are listed.
pub fn plan() -> Vec<Item> {
    let mut items = existing_data(&app_dirs::homes());
    if let Some(exe) = current_exe() {
        items.extend(links_to(&exe));
        items.push(program(exe));
    }
    items
}

fn existing_data(homes: &[PathBuf]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for home in homes {
        for (path, what) in app_dirs::data_locations(home) {
            if path.symlink_metadata().is_ok() && !items.iter().any(|i| i.path == path) {
                items.push(Item { path, what, kind: Kind::Data });
            }
        }
    }
    items
}

/// The running executable, with symlinks resolved on Unix (so a
/// `/usr/local/bin/cellar` link leads to the app).
fn current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Windows' canonical form is a `\\?\C:\...` path; keep the plain one.
    if cfg!(windows) { Some(exe) } else { exe.canonicalize().ok() }
}

fn program(exe: PathBuf) -> Item {
    // macOS: `Cellar.app/Contents/MacOS/cellar` — remove the whole bundle.
    match exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
        Some(bundle) => Item { path: bundle.to_path_buf(), what: "the Cellar app", kind: Kind::Program },
        None => Item { path: exe, what: "the cellar program", kind: Kind::Program },
    }
}

/// `cellar` links in the usual command folders that point at `exe`, as
/// created by the install instructions.
fn links_to(exe: &Path) -> Vec<Item> {
    if !cfg!(unix) {
        return Vec::new();
    }
    let mut dirs = vec![PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/homebrew/bin")];
    for home in app_dirs::homes() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join("bin"));
    }
    dirs.into_iter()
        .map(|d| d.join("cellar"))
        .filter(|link| link.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()))
        .filter(|link| link.canonicalize().is_ok_and(|target| target == exe))
        .map(|path| Item { path, what: "terminal command", kind: Kind::Link })
        .collect()
}

/// Remove one item. On Windows the running program can't delete itself;
/// it is deleted right after the process exits, so call this last.
pub fn remove(item: &Item) -> io::Result<()> {
    #[cfg(windows)]
    if item.kind == Kind::Program {
        return self_replace::self_delete_at(&item.path);
    }
    let meta = item.path.symlink_metadata()?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&item.path)
    } else {
        std::fs::remove_file(&item.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_stored_data_but_not_workbooks() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config").join("cellar");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("recent.json"), "[]").unwrap();
        let workbook = home.path().join("budget.cellar");
        std::fs::write(&workbook, "{}").unwrap();
        let other_app = home.path().join(".config").join("other");
        std::fs::create_dir_all(&other_app).unwrap();

        // The same home listed twice (HOME and USERPROFILE) is removed once.
        let homes = vec![home.path().to_path_buf(), home.path().to_path_buf()];
        let items = existing_data(&homes);
        assert_eq!(items.iter().map(|i| &i.path).collect::<Vec<_>>(), vec![&config]);

        for item in &items {
            remove(item).unwrap();
        }
        assert!(!config.exists());
        assert!(workbook.exists());
        assert!(other_app.exists());
    }

    #[test]
    fn program_inside_app_bundle_removes_the_bundle() {
        let exe = PathBuf::from("/Applications/Cellar.app/Contents/MacOS/cellar");
        assert_eq!(program(exe).path, PathBuf::from("/Applications/Cellar.app"));
        let exe = PathBuf::from("/home/u/.local/bin/cellar");
        assert_eq!(program(exe.clone()).path, exe);
    }
}
