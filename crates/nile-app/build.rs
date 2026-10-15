//! Regenerates `include/nile_app.h` from the exported functions.

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir");
    let config = cbindgen::Config::from_file(format!("{dir}/cbindgen.toml")).expect("cbindgen.toml");
    match cbindgen::Builder::new().with_crate(&dir).with_config(config).generate() {
        Ok(bindings) => {
            bindings.write_to_file(format!("{dir}/include/nile_app.h"));
        }
        Err(e) => println!("cargo:warning=could not generate nile_app.h: {e}"),
    }
}
