//! Bounded system preview capability, requested through the core.
#[cfg(target_os = "macos")]
pub fn thumbnail_bytes(name: &str, bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    use std::{
        io::Write,
        os::unix::fs::DirBuilderExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let path = std::env::temp_dir().join(format!("zork-file-preview-{}", ulid::Ulid::new()));
    std::fs::DirBuilder::new().mode(0o700).create(&path).ok()?;
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Temporary(path);
    let extension = std::path::Path::new(name).extension()?.to_str()?;
    if !extension.bytes().all(|b| b.is_ascii_alphanumeric()) || extension.len() > 16 {
        return None;
    }
    if matches!(
        extension.to_ascii_lowercase().as_str(),
        "zip" | "dmg" | "7z" | "rar" | "tar" | "gz" | "exe"
    ) {
        return None;
    }
    let source = directory.0.join(format!("preview.{extension}"));
    std::fs::File::create(&source).ok()?.write_all(bytes).ok()?;
    let mut child = Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", &side.to_string(), "-o"])
        .arg(&directory.0)
        .arg(&source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    match zork_notify::process::wait(&mut child, Instant::now() + Duration::from_secs(3)) {
        Ok(Some(status)) if status.success() => {}
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    }
    let result = directory.0.join(format!("preview.{extension}.png"));
    if result.metadata().ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    std::fs::read(result).ok()
}
#[cfg(not(target_os = "macos"))]
pub fn thumbnail_bytes(_: &str, _: &[u8], _: u32) -> Option<Vec<u8>> {
    None
}
