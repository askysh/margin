#!/usr/bin/env bash
# Build margin.exe for Windows and put the DLLs it needs at run time next to it.
# Run it in Git Bash. scripts/package-msix.ps1 then makes the Store package from it.
#
#   scripts/build-windows.sh              # the Store build: release, Whisper on the GPU,
#                                         # in src-tauri/target/vulkan/release
#   scripts/build-windows.sh --cpu        # release, Whisper on the CPU only,
#                                         # in src-tauri/target/release
#   scripts/build-windows.sh --debug      # debug, CPU only, with the UI built in
#   scripts/build-windows.sh --installer  # the Store build, then an installer for the
#                                         # current user, in release/windows. Add --cpu
#                                         # for an installer of the CPU build.
#
# It needs Rust (MSVC), Visual Studio with the C++ workload, CMake, LLVM and bun
# (docs/development.md, "Windows"). The Store build also needs the Vulkan SDK and the
# Vulkan runtime files, with VULKAN_SDK set (docs/development.md, "Whisper on the GPU").
#
# Variables for --installer:
#   MARGIN_UPDATER_KEY_PATH           the updater's private key. With it, the script
#   MARGIN_UPDATER_KEY_PASSWORD_PATH  signs the setup exe and writes its .sig, which the
#                                     in-app update needs (as in scripts/release.sh).
#   MARGIN_TAURI_CONFIG               a JSON file that Tauri merges into its config, for
#                                     tests only: another public key or version.
set -euo pipefail
cd "$(dirname "$0")/.."

profile=release
flags=()
target=src-tauri/target
gpu=1
installer=
for option in "$@"; do
  case "$option" in
    --debug)
      profile=debug
      flags=(--debug)
      gpu=
      ;;
    --cpu)
      gpu=
      ;;
    --installer)
      installer=1
      ;;
    *)
      echo "Unknown option: $option. Use --cpu, --debug, --installer or no option." >&2
      exit 1
      ;;
  esac
done
if [ -n "$installer" ] && [ "$profile" = debug ]; then
  echo "--installer makes a release build. Do not use it with --debug." >&2
  exit 1
fi
# Check the signing key before the long build, not after it.
sign=
if [ -n "$installer" ] && [ -n "${MARGIN_UPDATER_KEY_PATH:-}${MARGIN_UPDATER_KEY_PASSWORD_PATH:-}" ]; then
  if [ ! -f "${MARGIN_UPDATER_KEY_PATH:-}" ] || [ ! -f "${MARGIN_UPDATER_KEY_PASSWORD_PATH:-}" ]; then
    echo "Set MARGIN_UPDATER_KEY_PATH and MARGIN_UPDATER_KEY_PASSWORD_PATH to two files, or neither." >&2
    exit 1
  fi
  sign=1
fi
extra=()
if [ -n "${MARGIN_TAURI_CONFIG:-}" ]; then
  extra=(--config "$MARGIN_TAURI_CONFIG")
fi

loader=""
if [ -n "$gpu" ]; then
  if [ ! -f "${VULKAN_SDK:-}/Lib/vulkan-1.lib" ]; then
    echo "Set VULKAN_SDK to the folder of the Vulkan SDK. It has Lib/vulkan-1.lib." >&2
    echo "For a build with no GPU support, use --cpu." >&2
    exit 1
  fi
  # The Vulkan loader, vulkan-1.dll. margin.exe imports it, and a PC gets it only from a
  # graphics driver. So the app ships its own copy (DECISIONS.md D63). It is in the
  # "Runtime components" zip of the same SDK release, not in the SDK itself.
  loader="$(cygpath -u "${VULKAN_RUNTIME:-$VULKAN_SDK/runtime}")"
  if [ ! -f "$loader/x64/vulkan-1.dll" ] || [ ! -f "$loader/VulkanRT-License.txt" ]; then
    echo "The Vulkan runtime files are not in $loader." >&2
    echo "Unzip VulkanRT-X64-<version>-Components.zip from LunarG there, or set VULKAN_RUNTIME." >&2
    exit 1
  fi
  flags=(--features vulkan)
  # Its own folder, so the CPU build stays as it is. whisper.cpp builds its shader tool
  # in a project inside the project, and those paths go past the 260 characters that
  # MSBuild allows, from any folder with a normal path. So give the folder a drive
  # letter for the time of the build. `subst` needs no administrator rights.
  target=src-tauri/target/vulkan
  drive="${MARGIN_VULKAN_DRIVE:-V}"
  mkdir -p "$target"
  if [ -e "/${drive,,}/" ]; then
    echo "Drive $drive: is in use. Set MARGIN_VULKAN_DRIVE to a free letter." >&2
    exit 1
  fi
  cmd //c subst "$drive:" "$(cygpath -w "$PWD/$target")"
  trap 'cmd //c subst "$drive:" //D >/dev/null' EXIT
  export CARGO_TARGET_DIR="$drive:\\"
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
bunx tauri build "${flags[@]}" "${extra[@]}" --no-bundle

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

beside="DirectML.dll msvcp140.dll msvcp140_1.dll vcruntime140.dll vcruntime140_1.dll"
if [ -n "$gpu" ]; then
  cp -f "$loader/x64/vulkan-1.dll" "$loader/VulkanRT-License.txt" "$out/"
  beside="$beside vulkan-1.dll VulkanRT-License.txt"
fi

echo "Built: $out/margin.exe"
echo "Beside it: $beside"

if [ -z "$installer" ]; then
  exit 0
fi

# The installer: an NSIS setup from Tauri, for the current user, with no administrator
# rights. tauri.windows.conf.json has its settings. Tauri gets NSIS by itself the first
# time. The DLLs go in as resources at the top of the install folder, so the installed
# app has the same files beside its exe as the Store package.
resources=""
for file in $beside; do
  resources="$resources${resources:+, }\"$(cygpath -m "$PWD/$out/$file")\": \"$file\""
done
config="$out/installer.conf.json"
echo "{\"bundle\": {\"resources\": {$resources}}}" >"$config"
bunx tauri bundle "${flags[@]}" "${extra[@]}" --bundles nsis --config "$config"

# The version of the bundle: tauri.conf.json, or the one in MARGIN_TAURI_CONFIG.
version="$(bun -e 'const [base, extra] = process.argv.slice(1); const v = (extra && require(require("path").resolve(extra)).version) || require(require("path").resolve(base)).version; console.log(v)' src-tauri/tauri.conf.json "${MARGIN_TAURI_CONFIG:-}")"
setup="$out/bundle/nsis/Margin Notes_${version}_x64-setup.exe"
if [ ! -f "$setup" ]; then
  echo "The installer is not at $setup." >&2
  exit 1
fi
mkdir -p release/windows
final="release/windows/MarginNotes-setup-$version.exe"
cp -f "$setup" "$final"
rm -f "$final.sig"
echo "Installer: $final"

# The in-app update downloads this same exe. The app installs it only when the signature
# matches the public key in tauri.conf.json.
if [ -n "$sign" ]; then
  bunx tauri signer sign -f "$MARGIN_UPDATER_KEY_PATH" -p "$(cat "$MARGIN_UPDATER_KEY_PASSWORD_PATH")" "$final" >/dev/null
  if [ ! -s "$final.sig" ]; then
    echo "The installer was not signed." >&2
    exit 1
  fi
  echo "Signature: $final.sig"
fi
