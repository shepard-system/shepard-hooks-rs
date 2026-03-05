use std::path::Path;
use std::process::Command;

pub struct GitContext {
    pub repo: String,
    #[allow(dead_code)]
    pub branch: String,
}

/// Extract git repo name and branch from a working directory.
/// Falls back to "unknown" if git is not available or the path is not a repo.
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

    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    GitContext { repo, branch }
}
