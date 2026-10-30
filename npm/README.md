# nlayer npm packages

`@nlayer/cli` installs the `nlayer` binary. npm packs one package per platform
and the main package picks the one that matches the machine:

| Package | Binary |
| --- | --- |
| `@nlayer/cli` | `bin/nlayer.js`, `nlayer.wasm`, `scripts/postinstall.js` |
| `@nlayer/cli-darwin-arm64` | `bin/nlayer` |
| `@nlayer/cli-linux-x64` | `bin/nlayer` |
| `@nlayer/cli-win32-x64` | `bin/nlayer.exe` |

The main package lists the three in `optionalDependencies`, so npm installs
only the matching one and skips the others. `bin/nlayer.js` spawns that
binary, and sets `NLAYER_WASM` to the packaged `nlayer.wasm` unless the caller
already set it, because the web runtime is one file for every platform and
does not belong in each platform package.

The binary is built for macOS on Apple Silicon only. `darwin-x64` is not a
separate package: Rosetta 2 translates x86-64 for arm64, not the reverse.

## Versions

`Cargo.toml`'s `[workspace.package] version` is the only version. Run

```sh
node npm/scripts/sync-version.js
```

after bumping it. That writes the four npm packages and the five type packages
in `packages/`, whose versions `nlayer-bundler` asserts against the CLI.

## Try the layout locally

```sh
npm/scripts/test-local.sh
```

Builds the CLI and the wasm runtime for this machine, fills in the platform
package, syncs versions, copies the license, and runs the wrapper's
`--version`.

## Release

`.github/workflows/release.yml` is manual: Actions → release → Run workflow.
`dry_run` defaults to true, which builds every platform, assembles the
packages, runs the version and layout check, and uploads the `.tgz` files as
the `npm-tarballs` artifact. Nothing reaches the registry.

`dry_run: false` publishes the platform packages first and `@nlayer/cli` last,
with `--provenance`, using `NPM_TOKEN`. The type packages are separate:
`scripts/publish-types` publishes those at the same version.

Tag triggers are commented out in the workflow until a release is routine.

### What the workflow checks

- every platform package has its binary, and the main package has
  `nlayer.wasm`;
- `@nlayer/cli`'s `optionalDependencies` versions equal its own;
- the five `packages/*` versions equal the CLI's.

`npm pack` stores the mode, so the `chmod +x` before packing is what makes the
installed binary executable. `actions/upload-artifact` does not keep modes,
which is why the publish job chmods again after downloading.
