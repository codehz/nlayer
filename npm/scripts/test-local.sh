#!/bin/sh
# Build the CLI for this machine, put it in its platform package, and run the
# wrapper. Proves the npm layout without publishing: `npm link` is not needed
# because the wrapper is spawned directly.
set -eu
cd "$(dirname "$0")/../.."

case "$(uname -s)-$(uname -m)" in
Darwin-arm64) platform=darwin-arm64 ;;
Linux-x86_64) platform=linux-x64 ;;
MINGW*-x86_64 | MSYS*-x86_64) platform=win32-x64 ;;
*)
  echo "test-local: unsupported platform $(uname -s)-$(uname -m)" >&2
  exit 1
  ;;
esac

binary=nlayer
[ "$platform" = win32-x64 ] && binary=nlayer.exe

cargo build --release -p nlayer-cli
cargo build -p nlayer-platform-web --target wasm32-unknown-unknown --release

mkdir -p "npm/cli-$platform/bin"
cp "target/release/$binary" "npm/cli-$platform/bin/$binary"
chmod +x "npm/cli-$platform/bin/$binary"
cp target/wasm32-unknown-unknown/release/nlayer_platform_web.wasm npm/cli/nlayer.wasm

node npm/scripts/sync-version.js
for dir in cli cli-darwin-arm64 cli-linux-x64 cli-win32-x64; do
  cp LICENSE-APACHE "npm/$dir/LICENSE"
done

# The wrapper resolves the platform package through node_modules, the way an
# installed CLI does. `npm link` is offline, so it works before publishing.
(cd npm/cli && npm link "../cli-$platform" >/dev/null)

# The release workflow fills `npm/runtime-*/runtime`; when it is here the
# wrapper links it too and the CLI runs the prebuilt runtime instead of
# building one from this checkout.
for runtime in runtime-android runtime-ios; do
  [ -d "npm/$runtime/runtime" ] || continue
  (cd npm/cli && npm link "../$runtime" >/dev/null)
done

echo
echo "wrapper reports:"
node npm/cli/bin/nlayer.js --version
