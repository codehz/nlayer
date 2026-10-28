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
    let tsconfig = fs::read_to_string(dir.join("tsconfig.json")).unwrap();
    assert!(tsconfig.contains("\"jsxImportSource\": \"@nlayer\""));
    assert!(
        tsconfig.contains(".nlayer/types/globals.d.ts"),
        "ES2022 has no fetch or console; the globals file has to be included"
    );
    assert!(
        tsconfig.contains("\"./.nlayer/types/@nlayer/runtime.d.ts\""),
        "paths must be relative: TS rejects non-relative paths without baseUrl"
    );
    assert!(
        fs::read_to_string(dir.join(".nlayer/types/@nlayer/ui.d.ts"))
            .unwrap()
            .contains("export function View")
    );

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

    fs::write(dir.join("App.tsx"), "export const kept = true;\n").unwrap();
    let again = init(&InitOptions { dir: dir.clone() }).expect("a second init");
    assert!(again.kept.iter().any(|path| path == "App.tsx"), "{again}");
    assert_eq!(
        fs::read_to_string(dir.join("App.tsx")).unwrap(),
        "export const kept = true;\n"
    );
    let _ = fs::remove_dir_all(&dir);
}
