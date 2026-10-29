//! Writes `packages/{dir}/{path}` from [`nlayer_bundler::package_files`].
//! Does not touch `package.json`, `README.md`, or `LICENSE`.

use std::{fs, path::Path};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (dir, path, contents) in nlayer_bundler::package_files() {
        let rel = format!("packages/{dir}/{path}");
        let to = root.join(&rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("create {}: {error}", parent.display());
            });
        }
        if fs::read(&to)
            .ok()
            .is_some_and(|bytes| bytes == contents.as_bytes())
        {
            continue;
        }
        fs::write(&to, &contents).unwrap_or_else(|error| {
            panic!("write {}: {error}", to.display());
        });
        println!("wrote {rel}");
    }
}
