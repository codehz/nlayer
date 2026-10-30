# native-layer

One TSX application, run on web, iOS and Android. Mechanisms are extracted
from [nsh](https://github.com/longbridge/nsh); existing nsh apps are not
compatible.

v1 is the `nlayer` CLI (`init` / `build` / `run --target web|ios|android`)
plus a TSX test runner: headless by default, and the same steps on a real web, iOS or Android host (`nlayer test --target`). An app entry instead of a test file is opened and the steps from standard input run, on that host too. The public surface is `View`, `Text`,
`Image`, `Scroll` and single-line `Input`, with the nsh reactive graph and
Taffy layout. `nlayer run --target web` serves an app to a browser on this
machine, rebuilds when its TSX or assets change, and reloads the page; a
failed rebuild is printed and the previous page keeps running. `nlayer run
--target ios` installs a simulator host once, then restarts that process
after a rebuild without reinstalling it. See `NOTICE`
for the Apache-2.0 provenance of copied code.

Editor types are the npm packages `@nlayer/runtime`, `@nlayer/ui`, `@nlayer/jsx-runtime`, `@nlayer/test` and `@nlayer/globals`, versioned with `nlayer --version`. `nlayer init` writes `package.json` and a `nlayer.json` with a placeholder app ID; `bun install` fetches the editor types. The build does not need them.

`nlayer package --target web` writes a static site to `.nlayer/package/web`;
`--out` selects another directory. Imported images are included under content
hashes. Other files fetched by the app must be deployed separately. The web
runtime is loaded from `NLAYER_WASM`, next to the CLI, or built from this workspace.

`nlayer package --target ios` writes an unsigned device `.app` at
`.nlayer/package/Nlayer.app`. It requires Xcode and the Rust
`aarch64-apple-ios` target. Use `--out` for a new output path; signing and
installation are performed separately.

Licensed under the [Apache License, Version 2.0](LICENSE-APACHE).

v2 is planned in [`docs/v2.md`](docs/v2.md): a virtualized `List`, animated
values and long press, `resource` and `storage`, the system color scheme and
accessibility props, multiline input with selection and IME events, the
system back button, and unsigned `nlayer package` for web, Android and iOS.
