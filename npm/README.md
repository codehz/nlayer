# nlayer npm packages

`@nlayer/cli` installs the `nlayer` binary. npm packs one package per platform
and the main package picks the one that matches the machine:

| Package | Contents |
| --- | --- |
| `@nlayer/cli` | `bin/nlayer.js`, `nlayer.wasm`, `scripts/postinstall.js` |
| `@nlayer/cli-darwin-arm64` | `bin/nlayer` |
| `@nlayer/cli-linux-x64` | `bin/nlayer` |
| `@nlayer/cli-win32-x64` | `bin/nlayer.exe` |
| `@nlayer/runtime-android` | `runtime/android/classes.dex`, `runtime/android/lib/<abi>/libnlayer.so` |
| `@nlayer/runtime-ios` | `runtime/ios/iphonesimulator/nlayer-ios-app`, `runtime/ios/iphoneos/nlayer-ios-app` |

The main package lists all five in `optionalDependencies`, so npm installs
only the matching platform one and skips the others; `@nlayer/runtime-ios`
declares `os: ["darwin"]`, so npm skips it elsewhere. `bin/nlayer.js` spawns
that binary, and sets `NLAYER_WASM` to the packaged `nlayer.wasm` unless the
caller already set it, because the web runtime is one file for every platform
and does not belong in each platform package. It sets `NLAYER_RUNTIME_ANDROID`
and `NLAYER_RUNTIME_IOS` to the runtime directories it resolves through
`node_modules`, the same way: the Android engine is built for Android ABIs and
the iOS hosts for iOS, so neither depends on the host that runs the CLI.
Without the packages the CLI builds the runtime from a cargo checkout instead.

The binary is built for macOS on Apple Silicon only. `darwin-x64` is not a
separate package: Rosetta 2 translates x86-64 for arm64, not the reverse.

## Versions

`Cargo.toml`'s `[workspace.package] version` is the only version.

```sh
scripts/bump 0.3.0      # or major / minor / patch
```

writes it into `Cargo.toml`, the six `nlayer-*` workspace dependencies and the
matching `Cargo.lock` entries, then copies it to the six npm packages and the
five type packages in `packages/`, whose versions `nlayer-bundler` asserts
against the CLI. It edits files only: committing and tagging stay manual.

`npm/scripts/sync-version.js` does just the copy from the current `Cargo.toml`,
which is what CI runs after assembling the packages.

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

`dry_run: false` stages the version in `Cargo.toml`: the five type packages in
`packages/` first, then the runtime and platform packages, then `@nlayer/cli`
last (it names the other five in `optionalDependencies`). The dry run packs
the same eleven, so `npm-tarballs` shows exactly what would be staged.

There is no separate type-package release: the versions are one number, and a
run that skipped them would leave editor types behind the CLI.

A package whose version is already on the registry is skipped with a message,
so the type packages published before this workflow existed do not fail the
first CLI release. A version that is staged but not yet approved is not
skipped: reject it before staging again.

Tag triggers are commented out in the workflow until a release is routine.

### Auth: trusted publishing, staged

Nothing the workflow does goes live. It runs `npm stage publish`, which puts
each tarball in npm's staging area; a maintainer then approves or rejects it
with 2FA. That review is the reason for the step: a wrong binary never reaches
an installer.

[Staged publishing][sp] and [trusted publishing][tp] both need npm CLI 11.15.0
or later and Node 22.14 or later. The job installs Node 24 and `npm@11`
(`npm@latest` is 12, which wants a newer Node than the runner has). npm
exchanges the workflow's OIDC token (`permissions: id-token: write`), so there
is no `NPM_TOKEN`; a token in the environment would take precedence over OIDC
and should not be added. Provenance comes from that path.

[sp]: https://docs.npmjs.com/staged-publishing
[tp]: https://docs.npmjs.com/trusted-publishers

`npm stage publish` itself needs no 2FA. Reviewing does:

```sh
npm stage list                 # pending versions and their ids
npm stage view <id>            # metadata
npm stage download <id>        # the tarball, to inspect
npm stage approve <id>         # 2FA, then it is live
npm stage reject <id>          # drop it
```

The same list and the Approve button are under "Staged Packages" on npmjs.com.
Approve the runtime and platform packages before `@nlayer/cli`, the same order
the workflow stages them in: the main package names them in
`optionalDependencies`.

The repository must be public, and it must be the one the workflow runs in:
`codehz/nlayer`.

### Before the first release

Trusted publishing is configured per package on npmjs.com, and a package that
does not exist has nowhere to put that configuration. Staging does create a
`0.0.0-stage` placeholder for a new package, but the workflow needs the trust
relationship before it can stage at all, so the nine packages have to exist
first:

- the five type packages were published at `0.1.0`;
- the four CLI packages were published once as `0.0.0` with
  `--tag placeholder`, so `latest` stays unset and `npm install -g
  @nlayer/cli` does not install an empty package.

Then, per package on npmjs.com → Settings → Trusted publishing:

| Field | Value |
| --- | --- |
| Organization or user | `codehz` |
| Repository | `nlayer` |
| Workflow filename | `release.yml` |
| Allowed actions | leave `npm publish` off |

`npm stage publish` is always allowed, so nothing needs ticking; leaving
`npm publish` and `npm dist-tag` off keeps the workflow to staging only. A new
configuration expires if nothing is staged or published within two days.

Once a package has gone through the workflow, set its Publishing access to
"Require two-factor authentication and disallow tokens". Trusted publishing
keeps working, and long-lived tokens stop being an option for it.

### What the workflow checks

- every platform package has its binary, and the main package has
  `nlayer.wasm`;
- `@nlayer/cli`'s `optionalDependencies` versions equal its own;
- the five `packages/*` versions equal the CLI's.

`npm pack` stores the mode, so the `chmod +x` before packing is what makes the
installed binary executable. `actions/upload-artifact` does not keep modes,
which is why the publish job chmods again after downloading.
