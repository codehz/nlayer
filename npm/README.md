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

`dry_run: false` publishes the version in `Cargo.toml`, the five type packages
in `packages/` first, then the three platform packages, then `@nlayer/cli`
last (it names the platform packages in `optionalDependencies`). The dry run
packs the same nine, so `npm-tarballs` shows exactly what would be uploaded.

There is no separate type-package release: the versions are one number, and a
run that skipped them would leave editor types behind the CLI.

A package whose version is already on the registry is skipped with a message,
so a run that fails halfway resumes, and the type packages published before
this workflow existed do not fail the first CLI release.

Tag triggers are commented out in the workflow until a release is routine.

### Auth: trusted publishing, no token

The publish job uses npm's [trusted publishing][tp]: it exchanges the
workflow's OIDC token (`permissions: id-token: write`) and npm generates
provenance automatically. There is no `NPM_TOKEN`, and none should be added —
a token set in the environment takes precedence over OIDC.

[tp]: https://docs.npmjs.com/trusted-publishers

Requirements, from that page:

- npm CLI 11.5.1 or later and Node 22.14 or later. The job installs Node 24
  and `npm@latest`, because the Node-bundled npm can be older than 11.5.1.
- The repository must be public, and it must be the one the workflow runs in:
  `codehz/nlayer`.

### Before the first release

Trusted publishing is configured per package on npmjs.com, and a package that
does not exist has nowhere to put that configuration. The nine packages
therefore need to exist first:

- the five type packages were published at `0.1.0` before this workflow;
- the four CLI packages were published once as `0.0.0`, with
  `--tag placeholder`, so `latest` stays unset and `npm install -g
  @nlayer/cli` does not install an empty package.

Then, per package on npmjs.com → Settings → Trusted publishing:

| Field | Value |
| --- | --- |
| Organization or user | `codehz` |
| Repository | `nlayer` |
| Workflow filename | `release.yml` |
| Allowed actions | must include `npm publish` |

A configuration created after 2026-09-03 allows `npm stage publish` only
unless `npm publish` is ticked; without it the workflow is rejected. A new
configuration also expires if nothing is published within two days.

Once a package has published through the workflow, set its Publishing access
to "Require two-factor authentication and disallow tokens"; trusted publishing
keeps working, and long-lived tokens stop being an option.

### What the workflow checks

- every platform package has its binary, and the main package has
  `nlayer.wasm`;
- `@nlayer/cli`'s `optionalDependencies` versions equal its own;
- the five `packages/*` versions equal the CLI's.

`npm pack` stores the mode, so the `chmod +x` before packing is what makes the
installed binary executable. `actions/upload-artifact` does not keep modes,
which is why the publish job chmods again after downloading.
