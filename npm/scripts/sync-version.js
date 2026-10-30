#!/usr/bin/env node
// The version of every npm package is the workspace version in Cargo.toml.
// `packages/*` must match it: `nlayer-bundler` asserts that in
// `package_versions_match_the_cli`.
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..", "..");

function workspaceVersion() {
  const lines = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8").split("\n");
  const start = lines.findIndex((line) => line.trim() === "[workspace.package]");
  if (start < 0) throw new Error("Cargo.toml has no [workspace.package]");
  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (line.startsWith("[")) break;
    const match = line.match(/^version\s*=\s*"([^"]+)"/);
    if (match) return match[1];
  }
  throw new Error("[workspace.package] has no version");
}

function rewrite(file, edit) {
  const before = fs.readFileSync(file, "utf8");
  const value = JSON.parse(before);
  edit(value);
  const after = JSON.stringify(value, null, 2) + "\n";
  if (after !== before) fs.writeFileSync(file, after);
}

const version = workspaceVersion();
const platforms = ["darwin-arm64", "linux-x64", "win32-x64"];

for (const name of ["cli", ...platforms.map((p) => `cli-${p}`)]) {
  rewrite(path.join(root, "npm", name, "package.json"), (value) => {
    value.version = version;
    if (value.optionalDependencies) {
      for (const dep of Object.keys(value.optionalDependencies)) {
        value.optionalDependencies[dep] = version;
      }
    }
  });
}

for (const name of ["runtime", "ui", "jsx-runtime", "test", "globals"]) {
  rewrite(path.join(root, "packages", name, "package.json"), (value) => {
    value.version = version;
  });
}

console.log(`npm and type packages: ${version}`);
