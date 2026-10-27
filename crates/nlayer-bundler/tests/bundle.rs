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
import { mount, View } from "@nlayer";
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
import { Text } from "@nlayer";

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
    })
    .expect("it bundles");
    // One module: the app's own import is gone, the native one stays.
    assert!(!out.code.contains("./Label"), "{}", out.code);
    assert!(out.code.contains("from \"@nlayer\""), "{}", out.code);
    assert!(out.code.contains("from \"nlayer:rt\""), "{}", out.code);
    assert!(!out.code.contains("<Text"), "JSX is compiled");
    assert!(!out.code.contains(": string"), "types are removed");
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
    })
    .expect_err("the import is missing");
    assert!(error.plain.contains("Missing"), "{}", error.plain);
}

#[test]
fn a_node_builtin_is_refused_by_name() {
    let dir = app(
        "builtin",
        &[("App.tsx", "import fs from \"fs\";\nexport const a = fs;\n")],
    );
    let error = bundle(&BundleOptions {
        entry: dir.join("App.tsx"),
        minify: false,
    })
    .expect_err("fs is not there");
    assert!(
        error.plain.contains("has no module `fs`"),
        "{}",
        error.plain
    );
}

#[test]
fn an_imported_image_is_an_asset_descriptor() {
    let dir = app(
        "image",
        &[(
            "App.tsx",
            r#"
import { mount, Image } from "@nlayer";
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
    })
    .expect("it bundles");
    assert!(out.code.contains("asset:/img/icon.png"), "{}", out.code);
    assert!(out.code.contains("scales: [1, 2]"), "{}", out.code);
    assert_eq!(
        out.assets,
        [PathBuf::from("img/icon.png"), PathBuf::from("img/icon@2x.png")]
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
    })
    .expect_err("SVG is not v1");
    assert!(error.plain.contains("not part of v1"), "{}", error.plain);
}
