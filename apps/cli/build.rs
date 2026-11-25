use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // A smoke report has to name the build it ran against, so the binary carries
    // its commit. A source tarball has no .git; "unknown" is honest there.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-env-changed=OPENCLICKY_COMMIT");

    let commit = std::env::var("OPENCLICKY_COMMIT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(git_commit)
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=OPENCLICKY_COMMIT={commit}");

    let built_at = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .filter(|s| s.parse::<u64>().is_ok())
        .unwrap_or_else(now_epoch);
    println!("cargo:rustc-env=OPENCLICKY_BUILT_AT={built_at}");
}

fn git_commit() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

fn now_epoch() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
