fn main() {
    // minutes-core links a static Swift bridge whose Swift Concurrency dylib lives only in the
    // dyld shared cache. Core's own rpath link-arg does not reach downstream binaries.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bin=margin=-Wl,-rpath,/usr/lib/swift");
        build_speech_helper();
        build_calendar_helper();
    }
    tauri_build::build()
}

/// Compiles minutes-core's EventKit helper, `calendar-events`, from the vendored core
/// (`../vendor/minutes-core/scripts`, D80) to `binaries/calendar-events-<target>`, so Tauri
/// bundles and signs it next to `margin`. Core looks for the helper beside the running binary
/// first. Without it, a copy of Margin on any other Mac reads no calendar (D67). Core itself
/// comes from vendor/ through Cargo's `[patch]`, so cargo's git checkout of the pinned rev
/// need not exist; the vendored scripts are that rev's files, unchanged.
fn build_calendar_helper() {
    let root = std::path::Path::new("../vendor/minutes-core");
    let source = root.join("scripts/calendar-events.swift");
    let plist = root.join("scripts/calendar-helper-Info.plist");
    assert!(source.is_file() && plist.is_file(), "calendar helper sources not found under {}", root.join("scripts").display());
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", plist.display());
    let target = std::env::var("TARGET").expect("TARGET");
    let arch = target.split('-').next().unwrap_or("arm64").replace("aarch64", "arm64");
    std::fs::create_dir_all("binaries").expect("create binaries/");
    let out = format!("binaries/calendar-events-{target}");
    let status = std::process::Command::new("xcrun")
        .args(["swiftc", "-O", "-target", &format!("{arch}-apple-macos14.4")])
        .args(["-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist", "-Xlinker"])
        .arg(&plist)
        .arg(&source)
        .args(["-o", &out])
        .status()
        .expect("run xcrun swiftc for calendar-events");
    assert!(status.success(), "swiftc failed for {}", source.display());
}

/// Compiles `margin-speech`, the Apple Speech live transcript helper (applespeech.rs, D56),
/// to `binaries/margin-speech-<target>`. Tauri bundles and signs it (`bundle.externalBin`),
/// and copies it next to the `margin` binary in a dev build. It needs the macOS 26 SDK
/// (Xcode 26 or later), as minutes-core does. The helper checks the macOS version at run
/// time, so Margin still runs on macOS 14.
fn build_speech_helper() {
    let source = "speech/margin-speech.swift";
    println!("cargo:rerun-if-changed={source}");
    let target = std::env::var("TARGET").expect("TARGET");
    let arch = target.split('-').next().unwrap_or("arm64").replace("aarch64", "arm64");
    std::fs::create_dir_all("binaries").expect("create binaries/");
    let out = format!("binaries/margin-speech-{target}");
    let status = std::process::Command::new("xcrun")
        .args(["swiftc", "-O", "-parse-as-library", "-target", &format!("{arch}-apple-macos14.4"), source, "-o", &out])
        .status()
        .expect("run xcrun swiftc for margin-speech");
    assert!(status.success(), "swiftc failed for {source}");
}
