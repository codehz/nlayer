#!/usr/bin/env node
const { spawn } = require("child_process");
const { join } = require("path");
const { existsSync } = require("fs");

const PLATFORM_MAP = {
  "darwin-arm64": "@nlayer/cli-darwin-arm64",
  "linux-x64": "@nlayer/cli-linux-x64",
  "win32-x64": "@nlayer/cli-win32-x64",
};

function binaryPath() {
  const key = `${process.platform}-${process.arch}`;
  const pkgName = PLATFORM_MAP[key];
  if (!pkgName) {
    console.error(`nlayer: unsupported platform ${key}`);
    console.error("Supported: macOS on Apple Silicon, Linux x64, Windows x64.");
    console.error("Build from source instead: cargo install --path crates/nlayer-cli");
    process.exit(1);
  }
  try {
    const pkgDir = join(require.resolve(`${pkgName}/package.json`), "..");
    return join(pkgDir, "bin", process.platform === "win32" ? "nlayer.exe" : "nlayer");
  } catch {
    console.error(`nlayer: ${pkgName} is not installed, which ${key} needs.`);
    console.error("Reinstall: npm install -g @nlayer/cli");
    process.exit(1);
  }
}

// The web runtime is one wasm for every platform, so it ships with this
// package instead of once per platform package. `NLAYER_WASM` wins so an
// override still works.
const webRuntime = join(__dirname, "..", "nlayer.wasm");
const env = { ...process.env };
if (existsSync(webRuntime)) {
  env.NLAYER_WASM ??= webRuntime;
}

// The Android and iOS runtimes are separate packages: what they carry is the
// same on every host, so neither is repeated per platform package. The binary
// reads them from the variables; an override the caller already set wins.
function runtimeDir(pkgName, platform) {
  try {
    return join(require.resolve(`${pkgName}/package.json`), "..", "runtime", platform);
  } catch {
    return undefined;
  }
}
const androidRuntime = runtimeDir("@nlayer/runtime-android", "android");
if (androidRuntime) env.NLAYER_RUNTIME_ANDROID ??= androidRuntime;
const iosRuntime = runtimeDir("@nlayer/runtime-ios", "ios");
if (iosRuntime) env.NLAYER_RUNTIME_IOS ??= iosRuntime;

const child = spawn(binaryPath(), process.argv.slice(2), { stdio: "inherit", env });

child.on("error", (error) => {
  console.error(`nlayer: ${error.message}`);
  process.exit(1);
});
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 1);
});
