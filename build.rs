fn main() {
    println!("cargo:rerun-if-changed=assets/icon.res");
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-msvc")) {
        let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-bin=MyMeta={root}/assets/icon.res");
    }
}
