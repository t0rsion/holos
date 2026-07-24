use std::path::Path;
use std::process::Command;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let hash = vcs_info_hash(&manifest)
        .or_else(|| git_hash(&manifest))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=HOLOS_GIT_HASH={hash}");
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=HOLOS_BUILD_PROFILE={profile}");
}

/// Packaged crates carry the source commit in .cargo_vcs_info.json; prefer it
/// so builds outside a git checkout still report real provenance.
fn vcs_info_hash(manifest: &str) -> Option<String> {
    let text = std::fs::read_to_string(Path::new(manifest).join(".cargo_vcs_info.json")).ok()?;
    let i = text.find("\"sha1\":")? + "\"sha1\":".len();
    let rest = text[i..].trim_start().strip_prefix('"')?;
    let sha: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    (sha.len() >= 12).then(|| sha[..12].to_string())
}

fn git_hash(manifest: &str) -> Option<String> {
    let top = Command::new("git")
        .args(["-C", manifest, "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let top = String::from_utf8_lossy(&top.stdout).trim().to_string();
    // A crate unpacked inside some unrelated repository must not report that
    // repository's commit as its own.
    if Path::new(&top).canonicalize().ok()? != Path::new(manifest).canonicalize().ok()? {
        return None;
    }
    // Watch the resolved ref, not just HEAD: same-branch commits and amends
    // update the ref file, which HEAD alone does not reflect.
    println!("cargo:rerun-if-changed={top}/.git/HEAD");
    if let Ok(head) = std::fs::read_to_string(format!("{top}/.git/HEAD")) {
        if let Some(r) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed={top}/.git/{r}");
        }
    }
    if Path::new(&top).join(".git/packed-refs").exists() {
        println!("cargo:rerun-if-changed={top}/.git/packed-refs");
    }
    let out = Command::new("git")
        .args(["-C", manifest, "rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
