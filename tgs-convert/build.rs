use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=HOMEBREW_PREFIX");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    // rlottie-sys currently emits -lstdc++ unconditionally. Apple Clang builds
    // rlottie against libc++, so link it explicitly and use Homebrew GCC only
    // to supply the compatibility libstdc++ name expected by that upstream crate.
    println!("cargo:rustc-link-lib=c++");
    match brew_gcc_library_directory() {
        Some(path) => println!("cargo:rustc-link-search=native={}", path.display()),
        None => println!(
            "cargo:warning=macOS builds need Homebrew GCC for rlottie-sys; run: brew install gcc"
        ),
    }
}

fn brew_gcc_library_directory() -> Option<PathBuf> {
    let output = Command::new("brew")
        .args(["--prefix", "gcc"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let prefix = String::from_utf8(output.stdout).ok()?;
    let directory = PathBuf::from(prefix.trim()).join("lib/gcc/current");
    directory.is_dir().then_some(directory)
}
