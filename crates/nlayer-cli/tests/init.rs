//! `nlayer init` writes an app that `test` and `build` can run, and keeps
//! a file that is already there.

use std::fs;

use nlayer_cli::{BuildOptions, InitOptions, TestOptions, build, init, test};

#[test]
fn init_writes_an_app_that_builds_and_tests() {
    let dir = std::env::temp_dir().join(format!("nlayer-init-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let report = init(&InitOptions { dir: dir.clone() }).expect("it creates the app");
    assert!(
        report.wrote.iter().any(|path| path == "App.tsx"),
        "{report}"
    );
    assert!(
        report.wrote.iter().any(|path| path == "App.test.tsx"),
        "{report}"
    );
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("nlayer.json")).unwrap()).unwrap();
    assert_eq!(config["id"], "com.example.app");
    assert_eq!(config["version"], "1.0.0");
    assert_eq!(config["build"], 1);
    let tsconfig = fs::read_to_string(dir.join("tsconfig.json")).unwrap();
    assert!(tsconfig.contains("\"jsxImportSource\": \"@nlayer\""));
    assert!(!tsconfig.contains(".nlayer/types"), "{tsconfig}");
    assert!(!tsconfig.contains("paths"), "{tsconfig}");
    assert!(
        tsconfig.contains("\"./node_modules/@nlayer\""),
        "{tsconfig}"
    );
    assert!(tsconfig.contains("\"globals\""), "{tsconfig}");
    assert!(!dir.join(".nlayer/types").exists());
    let version = env!("CARGO_PKG_VERSION");
    let package = fs::read_to_string(dir.join("package.json")).unwrap();
    for name in [
        "@nlayer/globals",
        "@nlayer/jsx-runtime",
        "@nlayer/runtime",
        "@nlayer/ui",
        "@nlayer/test",
    ] {
        assert!(package.contains(name), "{package}");
        assert!(package.contains(version), "{package}");
    }
    assert!(report.to_string().contains("bun install"), "{report}");

    let tested = test(&TestOptions {
        path: dir.clone(),
        filter: None,
        target: None,
    })
    .expect("the test runs");
    assert!(tested.ok(), "{tested}");

    let out = dir.join("out");
    build(&BuildOptions {
        entry: dir.clone(),
        out: out.clone(),
        release: false,
    })
    .expect("it builds");
    let code = fs::read_to_string(out.join("bundle.js")).unwrap();
    assert!(code.contains("Count: "), "{code}");
    assert!(!out.join("types").exists());

    fs::write(dir.join("App.tsx"), "export const kept = true;\n").unwrap();
    fs::write(dir.join("nlayer.json"), "{\"id\":\"org.example.kept\"}\n").unwrap();
    let again = init(&InitOptions { dir: dir.clone() }).expect("a second init");
    assert!(again.kept.iter().any(|path| path == "App.tsx"), "{again}");
    assert!(
        again.kept.iter().any(|path| path == "nlayer.json"),
        "{again}"
    );
    assert_eq!(
        fs::read_to_string(dir.join("nlayer.json")).unwrap(),
        "{\"id\":\"org.example.kept\"}\n"
    );
    assert!(!again.to_string().contains("bun install"), "{again}");
    assert_eq!(
        fs::read_to_string(dir.join("App.tsx")).unwrap(),
        "export const kept = true;\n"
    );
    let _ = fs::remove_dir_all(&dir);
}
