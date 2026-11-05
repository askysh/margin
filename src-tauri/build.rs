fn main() {
    // minutes-core links a static Swift bridge whose Swift Concurrency dylib lives only in the
    // dyld shared cache. Core's own rpath link-arg does not reach downstream binaries.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bin=margin=-Wl,-rpath,/usr/lib/swift");
    }
    tauri_build::build()
}
