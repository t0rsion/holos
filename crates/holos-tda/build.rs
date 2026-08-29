use std::path::Path;
use std::process::Command;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-check-cfg=cfg(holos_repository_tests)");
    if Path::new(&manifest)
        .join("../holos-tda-check/Cargo.toml")
        .is_file()
    {
        println!("cargo:rustc-cfg=holos_repository_tests");
    }
    let hash = vcs_info_hash(&manifest)
        .or_else(|| git_hash(&manifest))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=HOLOS_GIT_HASH={hash}");
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=HOLOS_BUILD_PROFILE={profile}");
}

/// Read the source commit from .cargo_vcs_info.json, which packaged crates
/// carry. Builds outside a git checkout still report real provenance.
fn vcs_info_hash(manifest: &str) -> Option<String> {
    let text = std::fs::read_to_string(Path::new(manifest).join(".cargo_vcs_info.json")).ok()?;
    let i = text.find("\"sha1\":")? + "\"sha1\":".len();
    let rest = text[i..].trim_start().strip_prefix('"')?;
    let sha: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    (sha.len() >= 12).then(|| sha[..12].to_string())
}

fn git_hash(manifest: &str) -> Option<String> {
    // The crate sits below the workspace root, so the repository root is
    // never the manifest directory. Ask git whether it tracks this crate's
    // manifest instead. Sources extracted from a .crate archive are
    // untracked, so a crate unpacked inside an unrelated repository still
    // reports no hash.
    git_run(manifest, &["ls-files", "--error-unmatch", "Cargo.toml"])?;
    watch_head(manifest);
    let hash = git_run(manifest, &["rev-parse", "--short=12", "HEAD"])?;
    (!hash.is_empty()).then_some(hash)
}

/// Rerun the script when the checked-out commit changes. Watch the resolved
/// ref, not just HEAD. Commits and amends on the same branch update the ref
/// file, but leave HEAD unchanged.
fn watch_head(manifest: &str) {
    let Some(head) = git_file(manifest, "HEAD") else {
        return;
    };
    if let Ok(text) = std::fs::read_to_string(&head) {
        if let Some(r) = text.trim().strip_prefix("ref: ") {
            if let Some(reference) = git_file(manifest, r) {
                println!("cargo:rerun-if-changed={reference}");
            }
        }
    }
    println!("cargo:rerun-if-changed={head}");
    if let Some(packed) = git_file(manifest, "packed-refs") {
        println!("cargo:rerun-if-changed={packed}");
    }
}

/// Locate a file in the git directory. `rev-parse --git-path` resolves it
/// for linked worktrees too, where HEAD and packed-refs live apart. Returns
/// None when the file is absent, because cargo reruns the script forever on
/// a watched path that does not exist.
fn git_file(manifest: &str, name: &str) -> Option<String> {
    let path = git_run(manifest, &["rev-parse", "--git-path", name])?;
    let path = Path::new(manifest).join(path);
    if !path.is_file() {
        return None;
    }
    Some(path.to_str()?.to_string())
}

fn git_run(dir: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", dir]).args(args);
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
