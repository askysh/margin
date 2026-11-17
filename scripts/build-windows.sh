#!/usr/bin/env bash
# Build margin.exe for Windows and put the DLLs it needs at run time next to it.
# Run it in Git Bash. scripts/package-msix.ps1 then makes the Store package from it.
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
target=src-tauri/target
case "${1:-}" in
  --debug)
    profile=debug
    flags=(--debug)
    ;;
esac

export PATH="$HOME/.cargo/bin:$PATH"
# whisper.cpp needs CMake, and bindgen needs libclang. Their installers do not always put
# them on the PATH.
for dir in "/c/Program Files/CMake/bin" "/c/Program Files/LLVM/bin"; do
  [ -d "$dir" ] && PATH="$dir:$PATH"
done
if [ -z "${LIBCLANG_PATH:-}" ] && [ -f "/c/Program Files/LLVM/bin/libclang.dll" ]; then
  export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
fi

# whisper.cpp: two settings that the build of `whisper-rs-sys` gets wrong for a release
# build with Visual Studio. It passes each CMAKE_ and GGML_ variable to CMake.
#
# 1. The `cmake` crate replaces CMAKE_<LANG>_FLAGS_RELEASE with flags that have no
#    optimization level. Whisper then runs about 13 times slower (UPSTREAM-PATCHES.md #21).
#    These are CMake's own flags for Release. The dash form keeps Git Bash from reading
#    them as paths.
# 2. GGML_NATIVE=ON compiles for the CPU of the build PC. A build PC with AVX-512 would
#    give an app that stops on most PCs. OFF gives AVX2, FMA and F16C on every build PC.
export CMAKE_C_FLAGS_RELEASE="${CMAKE_C_FLAGS_RELEASE:--MD -O2 -Ob2 -DNDEBUG}"
export CMAKE_CXX_FLAGS_RELEASE="${CMAKE_CXX_FLAGS_RELEASE:--MD -O2 -Ob2 -DNDEBUG}"
export GGML_NATIVE="${GGML_NATIVE:-OFF}"

out="$target/$profile"
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
