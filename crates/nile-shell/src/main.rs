//! `nile`: opens an HTML file in a native window, or renders it headless. See `nile_shell::run`.

fn main() {
    let code = nile_shell::run(std::env::args().skip(1).collect());
    if code != 0 {
        std::process::exit(code);
    }
}
