fn main() {
    if let Err(message) = sovereign_boot_lifecycle::run_state_cli(std::env::args().skip(1)) {
        eprintln!("spore-boot-state: {message}");
        std::process::exit(2);
    }
}
