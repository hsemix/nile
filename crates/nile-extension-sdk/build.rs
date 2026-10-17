fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = cbindgen::Config::from_file(format!("{dir}/cbindgen.toml")).unwrap();
    let mut header = Vec::new();
    cbindgen::Builder::new()
        .with_crate(&dir)
        .with_config(config)
        .generate()
        .expect("generate extension ABI header")
        .write(&mut header);
    // cbindgen cannot prefix an `extern` declaration, so give the entry point its export macro
    // here: MSVC rejects a `dllexport` definition of a prototype that isn't (C2375).
    let header = String::from_utf8(header).unwrap();
    let marked = header.replace("extern NileStatus nile_extension_init(", "NILE_EXTENSION_EXPORT NileStatus nile_extension_init(");
    assert_ne!(header, marked, "the generated header no longer declares nile_extension_init as expected");
    // Only rewrite on change, so the file's timestamp (and dependants' rebuilds) stay put.
    let path = format!("{dir}/include/nile_extension.h");
    if std::fs::read_to_string(&path).map_or(true, |old| old != marked) {
        std::fs::write(&path, marked).unwrap();
    }
}
