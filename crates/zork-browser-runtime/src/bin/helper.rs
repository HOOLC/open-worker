use cef::*;
fn main() {
    let args = args::Args::new();
    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };
    #[cfg(target_os = "macos")]
    let _library = {
        let library = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
        assert!(library.load());
        library
    };
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let code = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    std::process::exit(code.max(0));
}
