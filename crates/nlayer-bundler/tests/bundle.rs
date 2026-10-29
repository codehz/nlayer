//! Bundling an app's modules into one runnable ES module.

use std::path::PathBuf;

use nlayer_bundler::{BundleOptions, bundle};

fn app(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nlayer-bundle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, source) in files {
        std::fs::write(dir.join(path), source).unwrap();
    }
    dir
}

/// A `w`×`h` PNG header: all the image size reader looks at.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    out.extend(w.to_be_bytes());
    out.extend(h.to_be_bytes());
    out.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    out
}

#[test]
fn the_entrys_modules_are_bundled() {
    let dir = app(
        "modules",
        &[
            (
                "App.tsx",
                r#"
import { mount } from "@nlayer/runtime";
import { View } from "@nlayer/ui";
import { Label } from "./Label";

function App() {
    return <View style={{ width: 10, height: 10 }}><Label value="hi" /></View>;
}
mount(App);
"#,
            ),
            (
                "Label.tsx",
                r#"
import { Text } from "@nlayer/ui";

export function Label(props: { value: string }) {
    return <Text>{props.value}</Text>;
}
"#,
            ),
        ],
    );
    let out = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect("it bundles");
    // One module: the app's own import is gone, the native one stays.
    assert!(!out.code.contains("./Label"), "{}", out.code);
    assert!(
        out.code.contains("from \"@nlayer/runtime\""),
        "{}",
        out.code
    );
    assert!(out.code.contains("from \"nlayer:rt\""), "{}", out.code);
    assert!(!out.code.contains("<Text"), "JSX is compiled");
    assert!(!out.code.contains(": string"), "types are removed");
    assert!(!out.code.contains("sourceMappingURL"), "{}", out.code);
    let map: serde_json::Value = serde_json::from_str(&out.source_map).expect("source map JSON");
    let sources: Vec<&str> = map["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .filter_map(|s| s.as_str())
        .collect();
    assert!(
        sources.iter().any(|s| s.ends_with("App.tsx")),
        "{sources:?}"
    );
    assert!(
        sources.iter().any(|s| s.ends_with("Label.tsx")),
        "{sources:?}"
    );
    assert!(
        map["mappings"].as_str().is_some_and(|m| !m.is_empty()),
        "{map}"
    );
    let host = nlayer_js::Host::new().unwrap();
    host.eval_source(&out.code).expect("the bundle runs");
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    assert!(!ops.is_empty(), "the app rendered");
}

#[test]
fn an_unknown_import_is_an_error_of_the_module() {
    let dir = app(
        "missing",
        &[(
            "App.tsx",
            "import { nope } from \"./Missing\";\nexport const a = nope;\n",
        )],
    );
    let error = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("the import is missing");
    assert!(error.plain.contains("Missing"), "{}", error.plain);
}

#[test]
fn a_node_builtin_stays_an_external_import() {
    for spec in ["fs", "node:fs", "path", "buffer"] {
        let dir = app(
            &format!("builtin-{spec}"),
            &[(
                "App.tsx",
                &format!("import m from \"{spec}\";\nexport const a = m;\n"),
            )],
        );
        let bundled = bundle(&BundleOptions {
            entry: dir.join("App.tsx"),
            minify: false,
            test: false,
        })
        .unwrap_or_else(|error| {
            panic!("{spec} must stay external, not be refused: {}", error.plain)
        });
        assert!(
            bundled.code.contains(&format!("from \"{spec}\"")),
            "{spec} was inlined or rewritten: {}",
            bundled.code
        );
    }
}

#[test]
fn a_bare_scope_and_the_jsx_types_module_are_refused() {
    let dir = app(
        "bare",
        &[(
            "App.tsx",
            "import { signal } from \"@nlayer\";\nexport const a = signal;\n",
        )],
    );
    let error = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("bare @nlayer is not a module");
    assert!(error.plain.contains("@nlayer/runtime"), "{}", error.plain);

    let dir = app(
        "jsx-types",
        &[(
            "App.tsx",
            "import { jsx } from \"@nlayer/jsx-runtime\";\nexport const a = jsx;\n",
        )],
    );
    let error = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("jsx-runtime is types only");
    assert!(error.plain.contains("types only"), "{}", error.plain);
}

#[test]
fn a_handwritten_renderer_import_is_refused() {
    for (name, source) in [
        (
            "import",
            "import { element } from \"nlayer:rt\";\nexport const a = element;\n",
        ),
        (
            "side-effect",
            "import \"nlayer:rt\";\nexport const a = 1;\n",
        ),
        ("export", "export { element } from \"nlayer:rt\";\n"),
        (
            "dynamic",
            "export const load = () => import(\"nlayer:rt\");\n",
        ),
        ("require", "export const rt = require(\"nlayer:rt\");\n"),
        (
            "url",
            "export const url = new URL(\"nlayer:rt\", import.meta.url);\n",
        ),
        (
            "type",
            "import type { element } from \"nlayer:rt\";\nexport const a = 1;\n",
        ),
    ] {
        let dir = app(&format!("rt-{name}"), &[("App.tsx", source)]);
        let error = bundle(&BundleOptions {
            entry: dir.join("App.tsx"),
            minify: false,
            test: false,
        })
        .expect_err("apps do not write nlayer:rt");
        assert!(
            error.plain.contains("renderer's module"),
            "{name}: {}",
            error.plain
        );
    }
}

#[test]
fn an_imported_image_is_an_asset_descriptor() {
    let dir = app(
        "image",
        &[(
            "App.tsx",
            r#"
import { mount } from "@nlayer/runtime";
import { Image } from "@nlayer/ui";
import icon from "./img/icon.png";

function App() {
    return <Image src={icon} />;
}
mount(App);
"#,
        )],
    );
    std::fs::create_dir_all(dir.join("img")).unwrap();
    std::fs::write(dir.join("img/icon.png"), png(20, 10)).unwrap();
    std::fs::write(dir.join("img/icon@2x.png"), png(40, 20)).unwrap();
    let out = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect("it bundles");
    assert!(out.code.contains("asset:/img/icon.png"), "{}", out.code);
    assert!(out.code.contains("scales: [1, 2]"), "{}", out.code);
    assert_eq!(
        out.assets,
        [
            PathBuf::from("img/icon.png"),
            PathBuf::from("img/icon@2x.png")
        ]
    );
    let host = nlayer_js::Host::new().unwrap();
    host.eval_source(&out.code).expect("the bundle runs");
    let mut ops = Vec::new();
    host.take_ops(&mut ops);
    let image = ops.iter().find_map(|op| match op {
        nlayer_core::Op::SetProp {
            prop: nlayer_core::Prop::Source(Some(source)),
            ..
        } => Some(source),
        _ => None,
    });
    let image = image.expect("the image's source");
    assert_eq!(&*image.uri, "asset:/img/icon.png");
    assert_eq!(
        image.size,
        Some(nlayer_core::geometry::Size::new(
            nlayer_core::geometry::Points(20.0),
            nlayer_core::geometry::Points(10.0)
        ))
    );
}

#[test]
fn an_svg_is_refused_as_later() {
    let dir = app(
        "svg",
        &[(
            "App.tsx",
            "import icon from './icon.svg';\nexport const a = icon;\n",
        )],
    );
    std::fs::write(dir.join("icon.svg"), "<svg/>").unwrap();
    let error = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("SVG is not v1");
    assert!(error.plain.contains("not part of v1"), "{}", error.plain);
}

#[test]
fn an_app_cannot_import_the_test_module() {
    let dir = app(
        "no-test-module",
        &[(
            "App.tsx",
            "import { test } from \"@nlayer/test\";\nexport const t = test;\n",
        )],
    );
    let error = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("apps do not import the test module");
    assert!(error.plain.contains("nlayer test"), "{}", error.plain);
}

#[test]
fn a_test_bundle_inlines_the_test_module() {
    let dir = app(
        "test-module",
        &[(
            "App.test.tsx",
            "import { test, finish } from \"@nlayer/test\";\ntest(\"a\", () => {});\nfinish(null);\n",
        )],
    );
    let out = bundle(&BundleOptions {
        entry: dir.join("App.test.tsx"),
        minify: false,
        test: true,
    })
    .expect("a test file bundles");
    assert!(out.code.contains("__nlayerTest"), "{}", out.code);
    assert!(!out.code.contains("from \"@nlayer/test\""), "{}", out.code);
}

#[test]
fn installed_type_packages_are_not_bundled() {
    let dir = app(
        "installed-types",
        &[(
            "App.tsx",
            "import { signal } from \"@nlayer/runtime\";\nimport { View } from \"@nlayer/ui\";\nexport const n = signal;\n",
        )],
    );
    let runtime = dir.join("node_modules/@nlayer/runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(
        runtime.join("package.json"),
        "{\"name\":\"@nlayer/runtime\",\"version\":\"0.1.0\"}\n",
    )
    .unwrap();
    std::fs::write(
        runtime.join("index.js"),
        "export const signal = \"SENTINEL_RUNTIME\";\n",
    )
    .unwrap();
    let test_pkg = dir.join("node_modules/@nlayer/test");
    std::fs::create_dir_all(&test_pkg).unwrap();
    std::fs::write(
        test_pkg.join("index.js"),
        "export const test = \"SENTINEL_TEST\";\n",
    )
    .unwrap();

    let out = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
        test: false,
    })
    .expect("native modules stay external");
    assert!(
        out.code.contains("from \"@nlayer/runtime\""),
        "{}",
        out.code
    );
    assert!(!out.code.contains("SENTINEL_RUNTIME"), "{}", out.code);

    std::fs::write(
        dir.join("TestImport.tsx"),
        "import { test } from \"@nlayer/test\";\nexport const t = test;\n",
    )
    .unwrap();
    let error = bundle(&BundleOptions {
        entry: dir.join("TestImport.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("app cannot import the test module");
    assert!(
        error
            .plain
            .contains("`@nlayer/test` is the test runner's module"),
        "{}",
        error.plain
    );

    std::fs::write(
        dir.join("Globals.tsx"),
        "import { x } from \"@nlayer/globals\";\nexport const y = x;\n",
    )
    .unwrap();
    let error = bundle(&BundleOptions {
        entry: dir.join("Globals.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("globals are ambient");
    assert!(error.plain.contains("ambient types"), "{}", error.plain);

    std::fs::write(
        dir.join("Nope.tsx"),
        "import { x } from \"@nlayer/nope\";\n",
    )
    .unwrap();
    let error = bundle(&BundleOptions {
        entry: dir.join("Nope.tsx"),
        minify: false,
        test: false,
    })
    .expect_err("unknown @nlayer module");
    assert!(error.plain.contains("is not a module"), "{}", error.plain);
}
