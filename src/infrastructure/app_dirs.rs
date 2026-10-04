//! Where Cellar keeps files for itself: settings, caches, state. Workbooks
//! belong to the user and live wherever they are saved.
//!
//! Everything Cellar stores for itself goes under [`config_dir`] or
//! [`cache_dir`], so `cellar --uninstall` (see `uninstall`) finds it. A
//! location that can't live there must be added to [`data_locations`].

use std::path::{Path, PathBuf};

/// macOS bundle identifier. Must match `CFBundleIdentifier` in
/// `.github/workflows/release.yml`; macOS keeps some per-app state under it.
pub const BUNDLE_ID: &str = "io.github.haderlka.cellar";

/// The user's home. Windows has no `$HOME` outside shells like Git Bash;
/// fall back to the user profile so the GUI started from Explorer works.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Settings and small state (recent files): `~/.config/cellar`.
pub fn config_dir() -> Option<PathBuf> {
    home().map(|h| config_dir_in(&h))
}

/// Data that may be lost at any time (rich clipboard): `~/.cache/cellar`.
pub fn cache_dir() -> Option<PathBuf> {
    home().map(|h| cache_dir_in(&h))
}

fn config_dir_in(home: &Path) -> PathBuf {
    home.join(".config").join("cellar")
}

fn cache_dir_in(home: &Path) -> PathBuf {
    home.join(".cache").join("cellar")
}

/// Every home Cellar may have used. On Windows `$HOME` (set by Git Bash)
/// and the user profile can differ, and Cellar may have run under either.
pub fn homes() -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = Vec::new();
    for home in ["HOME", "USERPROFILE"].iter().filter_map(std::env::var_os).map(PathBuf::from) {
        let resolved = |p: &PathBuf| p.canonicalize().unwrap_or_else(|_| p.clone());
        if !homes.iter().any(|h| resolved(h) == resolved(&home)) {
            homes.push(home);
        }
    }
    homes
}

/// Everything stored for Cellar under `home`, by Cellar or by the OS, with
/// a description for the user. The paths need not exist.
pub fn data_locations(home: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut locations = vec![
        (config_dir_in(home), "settings and recent files"),
        (cache_dir_in(home), "clipboard cache"),
    ];
    if cfg!(target_os = "macos") {
        let library = home.join("Library");
        locations.extend([
            (library.join("Preferences").join(format!("{BUNDLE_ID}.plist")), "macOS preferences"),
            (library.join("Saved Application State").join(format!("{BUNDLE_ID}.savedState")), "macOS window state"),
            (library.join("Caches").join(BUNDLE_ID), "macOS cache"),
            (library.join("HTTPStorages").join(BUNDLE_ID), "macOS web cache"),
        ]);
    }
    locations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_cache_are_uninstall_locations() {
        let home = Path::new("/home/u");
        let paths: Vec<PathBuf> = data_locations(home).into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&config_dir_in(home)));
        assert!(paths.contains(&cache_dir_in(home)));
    }
}
