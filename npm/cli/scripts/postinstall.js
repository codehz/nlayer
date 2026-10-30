#!/usr/bin/env node
// npm runs this after install. It must not fail an unsupported platform:
// the wrapper reports that with a message that names a way forward.
const { join } = require("path");
const { existsSync } = require("fs");

const PLATFORM_MAP = {
  "darwin-arm64": "@nlayer/cli-darwin-arm64",
  "linux-x64": "@nlayer/cli-linux-x64",
  "win32-x64": "@nlayer/cli-win32-x64",
};

const key = `${process.platform}-${process.arch}`;
const pkgName = PLATFORM_MAP[key];

if (!pkgName) {
  console.warn(`nlayer: ${key} is not supported; see \`nlayer\` for the options.`);
  process.exit(0);
}

try {
  const pkgDir = join(require.resolve(`${pkgName}/package.json`), "..");
  const binary = join(pkgDir, "bin", process.platform === "win32" ? "nlayer.exe" : "nlayer");
  if (!existsSync(binary)) {
    console.error(`nlayer: ${pkgName} has no binary at ${binary}.`);
    console.error("Reinstall: npm install -g @nlayer/cli");
    process.exit(1);
  }
} catch (error) {
  console.error(`nlayer: ${pkgName} did not install: ${error.message}`);
  console.error("Reinstall: npm install -g @nlayer/cli");
  process.exit(1);
}
