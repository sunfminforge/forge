//! Stamps the binary with what it was built from, as `REVIEW_APPROVE_VERSION`:
//! the release, and, in a git checkout, the commit it was built at
//! (`0.1.0+g1de9824`, `.dirty` for a tree with changes). With no git around
//! it, the release alone.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let release = env!("CARGO_PKG_VERSION");
    let version = match git(&["rev-parse", "--short", "HEAD"]) {
        Some(commit) => {
            let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|s| !s.is_empty());
            format!("{release}+g{commit}{}", if dirty { ".dirty" } else { "" })
        }
        None => release.to_string(),
    };
    println!("cargo:rustc-env=REVIEW_APPROVE_VERSION={version}");
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(dir) = git(&["rev-parse", "--path-format=absolute", "--git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/index");
    }
}
