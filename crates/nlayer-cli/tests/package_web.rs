//! Static packages retain app identity and resolve imported assets.

use std::{fs, process::Command};

#[test]
fn web_package_contains_hashed_assets_and_app_identity() {
    let dir = std::env::temp_dir().join(format!("nlayer-package-web-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend(1u32.to_be_bytes());
    png.extend(1u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    fs::write(dir.join("icon.png"), &png).unwrap();
    fs::write(dir.join("nlayer.json"), r#"{"id":"org.example.test"}"#).unwrap();
    fs::write(
        dir.join("App.tsx"),
        r#"
import { mount } from "@nlayer/runtime";
import { Image } from "@nlayer/ui";
import icon from "./icon.png";
mount(() => <Image src={icon} />);
"#,
    )
    .unwrap();
    let out = dir.join("site");
    let result = Command::new(env!("CARGO_BIN_EXE_nlayer"))
        .args(["package", "--out"])
        .arg(&out)
        .arg(&dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let page = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(page.contains("org.example.test"));
    let files: Vec<_> = fs::read_dir(out.join("assets"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(fs::read(&files[0]).unwrap(), png);
    assert!(page.contains(files[0].file_name().unwrap().to_str().unwrap()));
    assert!(out.join("nlayer.wasm").is_file());
    assert!(out.join("bundle.js.map").is_file());
    fs::remove_dir_all(dir).unwrap();
}
