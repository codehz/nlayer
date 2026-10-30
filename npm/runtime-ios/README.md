# @nlayer/runtime-ios

The iOS hosts the nlayer CLI runs and packages an app with: the simulator host
`nlayer run` installs and the device runner `nlayer package` copies into the
`.app`, so neither needs Rust.

## Install

An optional dependency of `@nlayer/cli` on macOS. `npm install -g @nlayer/cli`
fetches it, and the `nlayer` wrapper points `NLAYER_RUNTIME_IOS` at
`runtime/ios`. Installing this package on its own does nothing.

## Contents

```text
runtime/ios/iphonesimulator/nlayer-ios-app   aarch64-apple-ios-sim
runtime/ios/iphoneos/nlayer-ios-app          aarch64-apple-ios
```

## Requirements

macOS with Xcode: the simulator run needs a simulator runtime, and the device
runner is unsigned, so installing it on a device is the app's own step.

## Override

`NLAYER_RUNTIME_IOS` names another directory with this layout, which the CLI
reads instead.

## License

Apache-2.0
