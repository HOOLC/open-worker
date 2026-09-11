fn main() {
    println!("cargo:rerun-if-env-changed=CC_aarch64_linux_android");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    // Rust links cdylibs with -nodefaultlibs. C dependencies compiled by the
    // NDK may use outlined atomics, which must come from that NDK's compiler-rt
    // rather than remain unresolved until Android's dlopen.
    let compiler = std::env::var("CC_aarch64_linux_android")
        .expect("set the Android NDK compiler; see scripts/android/build.py");
    let result = std::process::Command::new(compiler)
        .arg("-print-libgcc-file-name")
        .output()
        .expect("query NDK compiler runtime");
    assert!(result.status.success(), "NDK compiler runtime query failed");
    let library = String::from_utf8(result.stdout).expect("compiler runtime path");
    let library = library.trim();
    assert!(
        std::path::Path::new(library).is_file(),
        "NDK compiler runtime missing"
    );
    println!("cargo:rustc-link-arg={library}");
    println!("cargo:rustc-link-arg=-Wl,--no-undefined");
}
