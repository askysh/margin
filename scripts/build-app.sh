#!/usr/bin/env bash
# Build Margin.app (unsigned) into src-tauri/target/release/bundle/macos/.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
bun install
bunx tauri build --bundles app
echo "Built: src-tauri/target/release/bundle/macos/Margin.app"
