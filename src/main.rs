fn main() {
    librebar::crash::install(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    if let Err(error) = receipts::cli::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
