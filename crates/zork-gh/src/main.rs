fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match zork_gh::zork_gh_main(&argv) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
