//! `cargo xtask` binary entry point; all logic lives in the library.

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    std::process::exit(xtask::run(&argv));
}
