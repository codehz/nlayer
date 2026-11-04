//! Development bundles: component proxies, keep, and patches.

use std::path::{Path, PathBuf};

use nlayer_bundler::{BundleOptions, HotPatch, bundle, bundle_hot};

fn app(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nlayer-hot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, source) in files {
        std::fs::write(dir.join(path), source).unwrap();
    }
    dir
}

fn options(entry: PathBuf) -> BundleOptions {
    BundleOptions {
        entry,
        minify: false,
        test: false,
    }
}

fn script(entry: &Path) -> String {
    bundle_hot(entry, false)
        .unwrap_or_else(|error| panic!("{}", error.plain))
        .script()
        .0
}

fn component_signature(code: &str, name: &str) -> String {
    let marker = "__nlayer_hot.component(";
    let name_lit = format!("\"{name}\"");
    let mut search = code;
    while let Some(at) = search.find(marker) {
        let part = &search[at..];
        let Some(name_at) = part.find(&name_lit) else {
            search = &search[at + marker.len()..];
            continue;
        };
        let after = &part[name_at + name_lit.len()..];
        let bytes = after.as_bytes();
        for i in 0..bytes.len().saturating_sub(17) {
            if bytes[i] == b'"'
                && bytes[i + 17] == b'"'
                && after[i + 1..i + 17].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return after[i + 1..i + 17].to_owned();
            }
        }
        search = &search[at + marker.len()..];
    }
    panic!("no component signature for {name} in {code}");
}

#[test]
fn an_uppercase_jsx_function_is_a_hot_component() {
    let dir = app(
        "component",
        &[(
            "App.tsx",
            r#"
import { mount } from "@nlayer/runtime";
import { View } from "@nlayer/ui";

function Screen() {
    return <View />;
}
mount(Screen);
"#,
        )],
    );
    let code = script(&dir.join("App.tsx"));
    assert!(code.contains("__nlayer_hot.component"), "{code}");
    assert!(code.contains("\"Screen\""), "{code}");
}

#[test]
fn a_top_level_context_keeps_its_identity() {
    let dir = app(
        "context",
        &[(
            "App.tsx",
            r#"
import { context } from "@nlayer/runtime";

const Theme = context(null);
export const value = Theme;
"#,
        )],
    );
    let code = script(&dir.join("App.tsx"));
    assert!(code.contains("__nlayer_hot.context"), "{code}");
    assert!(code.contains("\"Theme\""), "{code}");
}

#[test]
fn only_signal_and_create_nav_stack_are_kept() {
    let dir = app(
        "keep",
        &[(
            "App.tsx",
            r#"
import { signal, resource, computed, effect } from "@nlayer/runtime";
import { createNavStack } from "@nlayer/ui";

const count = signal(1);
const stack = createNavStack([]);
const data = resource(() => 1);
const doubled = computed(() => count());
effect(() => {
    doubled();
});
export { count, stack, data, doubled };
"#,
        )],
    );
    let code = script(&dir.join("App.tsx"));
    assert_eq!(code.matches("__nlayer_hot.keep(").count(), 2, "{code}");
    assert!(code.contains("\"count\""), "{code}");
    assert!(code.contains("\"stack\""), "{code}");
    assert!(!code.contains("\"data\""), "{code}");
    assert!(!code.contains("\"doubled\""), "{code}");
}

#[test]
fn mount_wraps_a_function_argument_as_a_component() {
    let dir = app(
        "mount",
        &[(
            "App.tsx",
            r#"
import { mount } from "@nlayer/runtime";
import { View } from "@nlayer/ui";

mount(() => <View />);
"#,
        )],
    );
    let code = script(&dir.join("App.tsx"));
    assert!(code.contains("__nlayer_hot.component"), "{code}");
    assert!(code.contains("\"mount#0\""), "{code}");
}

#[test]
fn a_component_signature_tracks_its_own_code_and_the_rest() {
    let source = |label: &str, body: &str| {
        format!(
            "import {{ mount }} from \"@nlayer/runtime\";\n\
             import {{ View }} from \"@nlayer/ui\";\n\
             const label = \"{label}\";\n\
             function App() {{ return <View testID=\"{body}\">{{label}}</View>; }}\n\
             mount(App);\n"
        )
    };
    let dir = app("sig", &[("App.tsx", &source("home", ""))]);
    let entry = dir.join("App.tsx");
    let base = component_signature(&script(&entry), "App");
    std::fs::write(&entry, source("away", "")).unwrap();
    let outside = component_signature(&script(&entry), "App");
    assert_ne!(
        outside, base,
        "code outside the component changes its signature"
    );
    std::fs::write(&entry, source("home", " ")).unwrap();
    let own = component_signature(&script(&entry), "App");
    assert_ne!(own, base, "the component's own code changes its signature");
    assert_ne!(own, outside);
}

#[test]
fn import_meta_hot_is_the_runtime_meta_object() {
    let dir = app(
        "meta",
        &[(
            "App.tsx",
            "import.meta.hot.accept();\nexport const ready = true;\n",
        )],
    );
    let entry = dir.join("App.tsx");
    let code = script(&entry);
    assert!(
        code.contains("__nlayer_hot.meta(") && code.contains(").accept()"),
        "{code}"
    );
    assert!(code.contains("App.tsx"), "{code}");
    assert!(!code.contains("import.meta.hot"), "{code}");
    let cold = bundle(&options(entry)).expect("a published bundle still builds");
    assert!(!cold.code.contains("__nlayer_hot"), "{}", cold.code);
    assert!(!cold.code.contains("nlayer:hot"), "{}", cold.code);
}

#[test]
fn a_published_bundle_is_not_hot_and_the_dev_script_defines_modules() {
    let dir = app(
        "plain",
        &[(
            "App.tsx",
            r#"
import { mount } from "@nlayer/runtime";
import { View } from "@nlayer/ui";

function App() {
    return <View />;
}
mount(App);
"#,
        )],
    );
    let entry = dir.join("App.tsx");
    let cold = bundle(&options(entry.clone())).expect("it bundles");
    assert!(!cold.code.contains("__nlayer_hot"), "{}", cold.code);
    assert!(!cold.code.contains("nlayer:hot"), "{}", cold.code);
    let hot = script(&entry);
    assert!(hot.contains("nlayer:hot"), "{hot}");
    assert!(hot.contains("__nlayer_hot.define("), "{hot}");
    assert!(hot.contains("__nlayer_hot.start("), "{hot}");
}

#[test]
fn a_patch_is_unchanged_an_update_or_a_reload() {
    let dir = app(
        "patch",
        &[
            (
                "App.tsx",
                r#"
import { mount } from "@nlayer/runtime";
import { View } from "@nlayer/ui";
import { Child } from "./Child";

function App() {
    return <View><Child /></View>;
}
mount(App);
"#,
            ),
            (
                "Child.tsx",
                r#"
import { View } from "@nlayer/ui";

export function Child() {
    return <View />;
}
"#,
            ),
        ],
    );
    let entry = dir.join("App.tsx");
    let first = bundle_hot(&entry, false).unwrap_or_else(|error| panic!("{}", error.plain));
    let again = bundle_hot(&entry, false).unwrap_or_else(|error| panic!("{}", error.plain));
    assert!(matches!(again.patch(&first), HotPatch::Unchanged));

    // The module keeps running: the patch names the chunk that changed.
    let (whole, _) = first.script();
    let require = "require(\"./";
    let at = whole
        .find(require)
        .unwrap_or_else(|| panic!("the entry requires the child: {whole}"));
    let rest = &whole[at + require.len()..];
    let end = rest.find('"').expect("the child's specifier ends");
    let child_id = &rest[..end];

    std::fs::write(
        dir.join("Child.tsx"),
        "import { Text } from \"@nlayer/ui\";\n\
         export function Child() {\n\
             return <Text>edited</Text>;\n\
         }\n",
    )
    .unwrap();
    let edited = bundle_hot(&entry, false).unwrap_or_else(|error| panic!("{}", error.plain));
    match edited.patch(&first) {
        HotPatch::Update { code, .. } => {
            assert!(
                code.contains(&format!("{child_id:?}: function")),
                "update should name the edited module: {code}"
            );
            assert!(
                !code.contains("__nlayer_hot.start("),
                "an update is not a full start: {code}"
            );
        }
        other => panic!("expected an update, got {other:?}"),
    }

    let child =
        bundle_hot(&dir.join("Child.tsx"), false).unwrap_or_else(|error| panic!("{}", error.plain));
    assert!(
        matches!(child.patch(&first), HotPatch::Reload),
        "a different entry module reloads"
    );
}

#[test]
fn an_app_cannot_import_the_hot_runtime() {
    let dir = app(
        "no-hot-import",
        &[(
            "App.tsx",
            "import \"nlayer:hot\";\nexport const ready = true;\n",
        )],
    );
    let error = bundle(&options(dir.join("App.tsx"))).expect_err("apps do not write nlayer:hot");
    assert!(
        error.plain.contains("development runtime"),
        "{}",
        error.plain
    );
}

#[test]
fn a_hot_test_bundle_inlines_the_test_module() {
    let dir = app(
        "hot-test-module",
        &[(
            "App.test.tsx",
            "import { test, finish } from \"@nlayer/test\";\ntest(\"a\", () => {});\nfinish(null);\n",
        )],
    );
    let entry = dir.join("App.test.tsx");
    let error = bundle_hot(&entry, false).expect_err("an app hot bundle refuses @nlayer/test");
    assert!(
        error
            .plain
            .contains("`@nlayer/test` is the test runner's module"),
        "{}",
        error.plain
    );

    // The plugin loads test_api.js (it calls `__nlayerTest`) as a real module,
    // so the specifier is not left as an import. The script still starts the
    // hot runtime.
    let hot = bundle_hot(&entry, true)
        .unwrap_or_else(|error| panic!("{}", error.plain))
        .script()
        .0;
    assert!(hot.contains("nlayer:hot"), "{hot}");
    assert!(hot.contains("__nlayerTest"), "{hot}");
    assert!(!hot.contains("from \"@nlayer/test\""), "{hot}");
    assert!(!hot.contains("require(\"@nlayer/test\""), "{hot}");
}
