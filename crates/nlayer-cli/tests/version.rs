//! `nlayer --version` is the version the type packages ship.

use std::process::Command;

#[test]
fn version_is_the_package_version() {
    let bin = env!("CARGO_BIN_EXE_nlayer");
    let output = Command::new(bin)
        .arg("--version")
        .output()
        .expect("nlayer --version");
    assert!(output.status.success(), "status {:?}", output.status);
    let text = String::from_utf8(output.stdout).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(text.trim(), format!("nlayer {version}"));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
