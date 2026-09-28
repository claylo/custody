fn main() {
    librebar::crash::install(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    if let Err(error) = custody::cli::run() {
        if custody::output::is_broken_pipe(&error) {
            return;
        }
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
