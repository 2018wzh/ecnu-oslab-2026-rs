fn main() {
    println!("cargo:rerun-if-env-changed=OSLAB_LINKER");
    let script = std::env::var("OSLAB_LINKER").expect("请使用 cargo xtask build");
    println!("cargo:rerun-if-changed={script}");
    println!("cargo:rustc-link-arg=-T{script}");
}
