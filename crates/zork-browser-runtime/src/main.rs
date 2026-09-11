//! Sandboxed, application-owned CEF runtime. Only the parent process owns the
//! pipe endpoints; no browser-control port or user-installed browser is used.
mod engine;
#[cfg(target_os = "macos")]
mod mac;
mod output;

use cef::*;
use std::io::{BufRead, Read, Write};

fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--cef-dir") {
        println!(
            "{}",
            cef::sys::get_cef_dir()
                .ok_or_else(|| anyhow::anyhow!("CEF distribution is missing"))?
                .display()
        );
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
        let args = args::Args::new();
        let code = execute_process(
            Some(args.as_main_args()),
            None::<&mut App>,
            std::ptr::null_mut(),
        );
        if code >= 0 {
            std::process::exit(code);
        }
    }
    let profile = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("missing browser profile"))?;
    std::fs::create_dir_all(&profile)?;
    let profile = std::fs::canonicalize(profile)?;
    let executable = std::env::current_exe()?.canonicalize()?;
    #[cfg(target_os = "macos")]
    let bundle = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or_else(|| anyhow::anyhow!("browser runtime must be inside its app bundle"))?;
    #[cfg(target_os = "macos")]
    let _library = {
        let loader = library_loader::LibraryLoader::new(&executable, false);
        anyhow::ensure!(loader.load(), "cannot load bundled CEF");
        loader
    };
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    #[cfg(target_os = "macos")]
    mac::initialize();
    let (input, writer): (Box<dyn Read + Send>, Box<dyn Write + Send>) = {
        #[cfg(target_os = "macos")]
        {
            let args: Vec<_> = std::env::args().collect();
            if let Some(index) = args.iter().position(|s| s == "--ipc") {
                let socket = std::os::unix::net::UnixStream::connect(
                    args.get(index + 1)
                        .ok_or_else(|| anyhow::anyhow!("missing IPC path"))?,
                )?;
                (Box::new(socket.try_clone()?), Box::new(socket))
            } else {
                (Box::new(std::io::stdin()), Box::new(std::io::stdout()))
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            (Box::new(std::io::stdin()), Box::new(std::io::stdout()))
        }
    };
    output::start(writer);
    engine::set_profile(&profile);
    let args = args::Args::new();
    let settings = Settings {
        // CEF otherwise resolves these against the outermost Zork.app, while
        // its framework and helpers belong to the nested ZorkBrowser.app.
        #[cfg(target_os = "macos")]
        main_bundle_path: bundle.to_string_lossy().as_ref().into(),
        #[cfg(target_os = "macos")]
        framework_dir_path: bundle
            .join("Contents/Frameworks/Chromium Embedded Framework.framework")
            .to_string_lossy()
            .as_ref()
            .into(),
        #[cfg(target_os = "macos")]
        browser_subprocess_path: bundle
            .join("Contents/Frameworks/ZorkBrowser Helper.app/Contents/MacOS/ZorkBrowser Helper")
            .to_string_lossy()
            .as_ref()
            .into(),
        #[cfg(not(target_os = "macos"))]
        browser_subprocess_path: executable
            .parent()
            .unwrap()
            .join(if cfg!(windows) {
                "zork-browser-helper.exe"
            } else {
                "zork-browser-helper"
            })
            .to_string_lossy()
            .as_ref()
            .into(),
        root_cache_path: profile.to_string_lossy().as_ref().into(),
        cache_path: profile.join("Default").to_string_lossy().as_ref().into(),
        persist_session_cookies: 1,
        windowless_rendering_enabled: 1,
        log_file: profile
            .join("runtime.log")
            .to_string_lossy()
            .as_ref()
            .into(),
        ..Default::default()
    };
    let mut app = engine::RuntimeApp::new();
    anyhow::ensure!(
        initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut()
        ) == 1,
        "CEF initialization failed"
    );
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(input).lines() {
            let Ok(line) = line else { break };
            let Ok(request) = serde_json::from_str(&line) else {
                break;
            };
            engine::post(request);
        }
        engine::post(serde_json::json!({"method":"Browser.close"}));
        // If the desktop disappears, a blocked CEF shutdown must not leave its
        // browser alive indefinitely. EOF on the inherited pipe is ownership loss.
        // Allow cookie flushing and normal shutdown before the final fallback.
        std::thread::sleep(std::time::Duration::from_secs(10));
        std::process::exit(1);
    });
    run_message_loop();
    engine::clear();
    shutdown();
    Ok(())
}
