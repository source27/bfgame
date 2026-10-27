//! Games compiled into the executable so `bfgame snake.bf` works from any
//! directory.  A real `.bf` file on disk always wins, which keeps the
//! edit-compile-run workflow intact.

use std::path::{Path, PathBuf};

pub fn source_for(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    match name.as_str() {
        "snake.bf" | "snake" => Some(include_str!("../games/snake.bf")),
        "tetris.bf" | "tetris" => Some(include_str!("../games/tetris.bf")),
        _ => None,
    }
}

/// A path with no directory component, or explicitly `.\name`.
fn is_simple_name(path: &Path) -> bool {
    match path.parent() {
        None => true,
        Some(parent) => parent.as_os_str().is_empty() || parent == Path::new("."),
    }
}

/// Resolve a CLI argument to an on-disk file.  If the file is not present,
/// try `games/<name>` (the development layout).  If neither exists, keep the
/// original path so the built-in fallback can handle it.
pub fn resolve(path: PathBuf) -> PathBuf {
    if path.is_file() {
        return path;
    }
    if is_simple_name(&path) {
        if let Some(name) = path.file_name() {
            let candidate = Path::new("games").join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    path
}

pub fn is_builtin_name(path: &Path) -> bool {
    is_simple_name(path) && source_for(path).is_some()
}
