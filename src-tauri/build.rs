fn main() {
    // minutes-core links a static Swift bridge whose Swift Concurrency dylib lives only in the
    // dyld shared cache. Core's own rpath link-arg does not reach downstream binaries.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bin=margin=-Wl,-rpath,/usr/lib/swift");
        build_speech_helper();
    }
    tauri_build::build()
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
