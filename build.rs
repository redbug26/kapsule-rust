fn main() {
    println!("cargo:rerun-if-changed=src/web_bridge.js");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        let bridge = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("src/web_bridge.js");
        println!("cargo:rustc-link-arg=--js-library={}", bridge.display());
    }
}
