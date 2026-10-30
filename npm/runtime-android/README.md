# @nlayer/runtime-android

The Android runtime the nlayer CLI packages an app with: the engine for every
packaged ABI and the Java host, so `nlayer run` and `nlayer package` never need
Rust.

## Install

An optional dependency of `@nlayer/cli`. `npm install -g @nlayer/cli` fetches
it, and the `nlayer` wrapper points `NLAYER_RUNTIME_ANDROID` at
`runtime/android`. Installing this package on its own does nothing.

## Contents

```text
runtime/android/classes.dex
runtime/android/lib/arm64-v8a/libnlayer.so
runtime/android/lib/armeabi-v7a/libnlayer.so
runtime/android/lib/x86_64/libnlayer.so
```

## Requirements

Assembling the APK still uses the Android SDK (`aapt2`, `d8`, `zipalign`,
`apksigner`); `nlayer run` also needs `adb` and a device or emulator.

## Override

`NLAYER_RUNTIME_ANDROID` names another directory with this layout, which the
CLI reads instead.

## License

Apache-2.0
