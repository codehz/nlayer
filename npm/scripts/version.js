// The version has one source: `[workspace.package] version` in Cargo.toml.
// These are the files that copy it, and `nlayer-bundler` asserts the type
// packages against the CLI in `package_versions_match_the_cli`.
const fs = require("node:fs");
const path = require("node:path");

const npmPackages = [
  "cli",
  "cli-darwin-arm64",
  "cli-linux-x64",
  "cli-win32-x64",
  "runtime-android",
  "runtime-ios",
];
const typePackages = ["runtime", "ui", "jsx-runtime", "test", "globals"];

/// Package directories, relative to the repository root.
function packageDirs() {
  return [
    ...npmPackages.map((name) => `npm/${name}`),
    ...typePackages.map((name) => `packages/${name}`),
  ];
}

function readWorkspaceVersion(root) {
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

/// Copies the Cargo.toml version into every package.json. Returns the files
/// it changed.
function syncVersions(root) {
  const version = readWorkspaceVersion(root);
  const changed = [];
  for (const dir of packageDirs()) {
    const file = path.join(root, dir, "package.json");
    const before = fs.readFileSync(file, "utf8");
    rewrite(file, (value) => {
      value.version = version;
      if (value.optionalDependencies) {
        for (const dep of Object.keys(value.optionalDependencies)) {
          value.optionalDependencies[dep] = version;
        }
      }
    });
    if (fs.readFileSync(file, "utf8") !== before) changed.push(dir);
  }
  return { version, changed };
}

module.exports = { packageDirs, readWorkspaceVersion, syncVersions };
