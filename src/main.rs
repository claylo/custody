fn main() {
    if let Err(error) = receipts::cli::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
