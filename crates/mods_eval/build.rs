fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for name in ["drip", "hostile"] {
        let wat = std::fs::read_to_string(format!("src/{name}.wat")).unwrap();
        std::fs::write(out.join(format!("{name}.wasm")), wat::parse_str(&wat).unwrap()).unwrap();
        println!("cargo:rerun-if-changed=src/{name}.wat");
    }
}
