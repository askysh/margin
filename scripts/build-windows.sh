#!/usr/bin/env bash
# Build margin.exe for Windows and put the DLLs it needs at run time next to it.
# Run it in Git Bash. It does not make the Store package (docs/windows-store.md).
#
#   scripts/build-windows.sh            # release: src-tauri/target/release/margin.exe
#   scripts/build-windows.sh --debug    # debug, with the UI built in
#
# It needs Rust (MSVC), Visual Studio with the C++ workload, CMake, LLVM and bun
# (docs/development.md, "Windows").
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
flags=()
if [ "${1:-}" = "--debug" ]; then
  profile=debug
  flags=(--debug)
fi

export PATH="$HOME/.cargo/bin:$PATH"
# whisper.cpp needs CMake, and bindgen needs libclang. Their installers do not always put
# them on the PATH.
for dir in "/c/Program Files/CMake/bin" "/c/Program Files/LLVM/bin"; do
  [ -d "$dir" ] && PATH="$dir:$PATH"
done
if [ -z "${LIBCLANG_PATH:-}" ] && [ -f "/c/Program Files/LLVM/bin/libclang.dll" ]; then
  export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
fi

out="src-tauri/target/$profile"
# ort-sys 2.0.0-rc.10 copies DirectML.dll to the third folder above its OUT_DIR, and to
# `examples` and `deps` in it. Before cargo 1.100 that folder was target/<profile>. Now it
# is target/<profile>/build, where those two folders do not exist, and the build script
# stops with an error when Windows lets it make links. Make the folders first. Then copy
# the DLL to the right place after the build.
mkdir -p "$out/build/examples" "$out/build/deps"

bun install
bunx tauri build "${flags[@]}" --no-bundle

if [ ! -f "$out/build/DirectML.dll" ]; then
  echo "DirectML.dll is not in $out/build. Look at the ort-sys build output." >&2
  exit 1
fi
cp -f "$out/build/DirectML.dll" "$out/DirectML.dll"

# margin.exe needs the Visual C++ runtime. A PC that never had it installed gives error
# 0xC0000135 and no window. Visual Studio has copies that an app can ship beside its exe.
vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
crt=""
if [ -x "$vswhere" ]; then
  vs="$("$vswhere" -latest -products '*' -property installationPath | tr -d '\r')"
  crt="$(ls -d "$(cygpath -u "$vs")"/VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT 2>/dev/null | sort | tail -1)"
fi
if [ -z "$crt" ]; then
  echo "The Visual C++ runtime folder of Visual Studio was not found." >&2
  exit 1
fi
for dll in msvcp140.dll msvcp140_1.dll vcruntime140.dll vcruntime140_1.dll; do
  cp -f "$crt/$dll" "$out/$dll"
done

echo "Built: $out/margin.exe"
echo "Beside it: DirectML.dll msvcp140.dll msvcp140_1.dll vcruntime140.dll vcruntime140_1.dll"
