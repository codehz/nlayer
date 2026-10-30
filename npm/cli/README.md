# @nlayer/cli

Create, build, test and run a TSX app on web, iOS and Android.

## Install

```sh
npm install -g @nlayer/cli
```

The binary is built for macOS on Apple Silicon, Linux x64 and Windows x64.
Other platforms build from source:

```sh
cargo install --path crates/nlayer-cli
```

## Use

```sh
nlayer init my-app          # App.tsx, package.json, nlayer.json, tsconfig.json
cd my-app
bun install                 # the @nlayer type packages

nlayer build                # bundle, source map and imported images
nlayer run --target web     # serves a browser and rebuilds on change
nlayer run --target ios     # macOS with Xcode: simulator, rebuilt in place
nlayer run --target android # emulator or device on `adb`
nlayer test                 # *.test.tsx on the headless engine
nlayer test --target web    # the same steps in a headless browser
nlayer package --target web # static site in .nlayer/package/web
```

`nlayer run --target web` and `nlayer package --target web` need the web
runtime. This package ships it as `nlayer.wasm` and the `nlayer` wrapper points
`NLAYER_WASM` at it, so no separate download is involved. Android packaging
needs the Android SDK; iOS packaging needs Xcode and the
`aarch64-apple-ios` Rust target.

## License

Apache-2.0
