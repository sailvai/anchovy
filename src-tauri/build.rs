use std::process::Command;

fn main() {
    link_clang_runtime();
    tauri_build::build()
}

/// llama.cpp's Metal code checks the macOS version with `@available`. Built
/// for macOS 14.4, clang turns those checks into calls to
/// `__isPlatformVersionAtLeast`, which lives in clang's own runtime library.
/// Rust links without it, so the app build would fail to link.
fn link_clang_runtime() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let output = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .expect("xcrun runs: the Xcode command line tools are needed to build Anchovy");
    let resources = String::from_utf8(output.stdout).expect("a UTF-8 path");
    println!(
        "cargo:rustc-link-search=native={}/lib/darwin",
        resources.trim()
    );
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
