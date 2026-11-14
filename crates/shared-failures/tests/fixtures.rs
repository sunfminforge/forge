//! The fixture tests of shared-failures (`test.py`, run by `./test`), against the
//! binary forge releases.

use std::process::Command;

#[test]
fn fixtures() {
    let here = env!("CARGO_MANIFEST_DIR");
    let status = Command::new("/bin/sh")
        .arg(format!("{here}/test"))
        .env("DETECTOR", env!("CARGO_BIN_EXE_shared-failures"))
        .status()
        .expect("sh runs ./test");
    assert!(status.success(), "a fixture test of shared-failures failed: {status}");
}
