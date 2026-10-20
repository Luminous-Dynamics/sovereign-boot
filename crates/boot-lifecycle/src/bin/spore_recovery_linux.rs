fn main() {
    match sovereign_boot_lifecycle::run_recovery_cli(std::env::args().skip(1)) {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("spore-recovery-linux: {message}");
            std::process::exit(2);
        }
    }
}
