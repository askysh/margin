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

/// Compiles minutes-core's EventKit helper, `calendar-events`, from the pinned core checkout
/// to `binaries/calendar-events-<target>`, so Tauri bundles and signs it next to `margin`.
/// Core looks for the helper beside the running binary first. Without it, a copy of
/// Margin on any other Mac reads no calendar (D67). The source stays in core: this compiles
/// it from cargo's checkout and copies nothing into this repository.
fn build_calendar_helper() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read Cargo.toml");
    let rev = manifest
        .lines()
        .find(|l| l.starts_with("minutes-core") && l.contains("rev = \""))
        .and_then(|l| l.split("rev = \"").nth(1))
        .and_then(|r| r.split('"').next())
        .expect("minutes-core rev in Cargo.toml");
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cargo"));
    let checkouts = cargo_home.join("git").join("checkouts");
    let short = &rev[..7.min(rev.len())];
    let root = std::fs::read_dir(&checkouts)
        .expect("cargo git checkouts")
        .flatten()
        .filter(|d| d.file_name().to_string_lossy().starts_with("minutes-"))
        .map(|d| d.path().join(short))
        .find(|p| p.join("scripts/calendar-events.swift").is_file())
        .unwrap_or_else(|| panic!("minutes-core checkout {short} with scripts/calendar-events.swift not found under {}", checkouts.display()));
    let source = root.join("scripts/calendar-events.swift");
    let plist = root.join("scripts/calendar-helper-Info.plist");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=Cargo.toml");
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
