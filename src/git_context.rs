use std::path::Path;
use std::process::Command;

pub struct GitContext {
    pub repo: String,
}

/// Extract git repo name from a working directory.
/// Falls back to empty string if git is not available or the path is not a repo.
pub fn get(cwd: &str) -> GitContext {
    let path = Path::new(cwd);

    let repo = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let full = String::from_utf8_lossy(&o.stdout).trim().to_string();
                full.rsplit('/').next().map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    GitContext { repo }
}
