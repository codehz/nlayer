//! `nlayer build` writes the bundle, its source map, imported images and types.

use std::fs;

use nlayer_cli::{BuildOptions, build};

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    out.extend(w.to_be_bytes());
    out.extend(h.to_be_bytes());
    out.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    out
}

#[test]
fn build_writes_the_bundle_map_asset_and_types() {
    let dir = std::env::temp_dir().join(format!("nlayer-build-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("img")).unwrap();
    let image = png(20, 10);
    fs::write(dir.join("img/icon.png"), &image).unwrap();
    fs::write(
        dir.join("App.tsx"),
        r#"
import { mount } from "@nlayer/runtime";
import { Image, View } from "@nlayer/ui";
import icon from "./img/icon.png";

function App() {
    return <View style={{ width: 10, height: 10 }}><Image src={icon} /></View>;
}
mount(App);
"#,
    )
    .unwrap();
    let out = dir.join("out");
    build(&BuildOptions {
        entry: dir.clone(),
        out: out.clone(),
        release: false,
    })
    .expect("it builds");

    let code = fs::read_to_string(out.join("bundle.js")).unwrap();
    assert!(code.contains("asset:/img/icon.png"), "{code}");
    assert!(!code.contains("sourceMappingURL"), "{code}");
    let map = fs::read_to_string(out.join("bundle.js.map")).unwrap();
    assert!(map.contains("App.tsx"), "{map}");
    assert_eq!(fs::read(out.join("img/icon.png")).unwrap(), image);

    let styles = fs::read_to_string(out.join("types/styles.d.ts")).unwrap();
    assert!(styles.contains("gridTemplateColumns?:"));
    assert!(!styles.contains("boxShadow?:"));
    let ui = fs::read_to_string(out.join("types/@nlayer/ui.d.ts")).unwrap();
    assert!(ui.contains("export function Image"));
    assert!(
        fs::read_to_string(out.join("types/@nlayer/runtime.d.ts"))
            .unwrap()
            .contains("export function signal")
    );
    assert!(
        fs::read_to_string(out.join("types/@nlayer/test.d.ts"))
            .unwrap()
            .contains("export function open")
    );
    let _ = fs::remove_dir_all(&dir);
}
