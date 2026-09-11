fn main() {
    println!("cargo:rerun-if-changed=native/modal-blur.m");
    println!("cargo:rerun-if-changed=native/liquid-field.m");
    println!("cargo:rerun-if-changed=native/liquid-field.metal");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let object = out.join("modal-blur.o");
    let arch = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    assert!(std::process::Command::new("/usr/bin/clang")
        .args([
            "-fobjc-arc",
            "-O2",
            "-arch",
            arch,
            "-c",
            "native/modal-blur.m",
            "-o"
        ])
        .arg(&object)
        .args(
            if std::env::var_os("CARGO_FEATURE_HEADLESS_BENCH").is_some() {
                vec!["-DZORK_MODAL_TESTING=1"]
            } else {
                Vec::new()
            }
        )
        .status()
        .unwrap()
        .success());
    let liquid = out.join("liquid-field.o");
    assert!(std::process::Command::new("/usr/bin/clang")
        .args([
            "-fobjc-arc",
            "-O2",
            "-arch",
            arch,
            "-c",
            "native/liquid-field.m",
            "-o"
        ])
        .arg(&liquid)
        .status()
        .unwrap()
        .success());
    let air = out.join("liquid-field.air");
    assert!(std::process::Command::new("xcrun")
        .args([
            "-sdk",
            "macosx",
            "metal",
            "-c",
            "native/liquid-field.metal",
            "-o"
        ])
        .arg(&air)
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("xcrun")
        .args(["-sdk", "macosx", "metallib"])
        .arg(air)
        .arg("-o")
        .arg(out.join("liquid-field.metallib"))
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("/usr/bin/libtool")
        .args(["-static", "-o"])
        .arg(out.join("libzork_modal_blur.a"))
        .arg(object)
        .arg(liquid)
        .status()
        .unwrap()
        .success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=zork_modal_blur");
    for framework in ["AppKit", "QuartzCore", "Metal"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
