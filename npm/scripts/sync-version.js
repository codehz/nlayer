#!/usr/bin/env node
// Copies the Cargo.toml version into the npm and type packages. Does not
// change the version; `scripts/bump` does that.
const path = require("node:path");
const { syncVersions } = require("./version");

const root = path.join(__dirname, "..", "..");
const { version, changed } = syncVersions(root);
for (const dir of changed) console.log(`${dir}/package.json: ${version}`);
console.log(`npm and type packages: ${version}`);
