fn main() {
    println!("cargo:rerun-if-env-changed=OSLAB_LINKER");
    println!("cargo:rerun-if-env-changed=OSLAB_USER_IMAGE");
    let image = std::env::var("OSLAB_USER_IMAGE").expect("请通过 xtask 构建用户镜像");
    println!("cargo:rerun-if-changed={image}");
    println!("cargo:rustc-env=OSLAB_USER_IMAGE={image}");
    let script = std::env::var("OSLAB_LINKER").expect("请使用 cargo xtask build");
    println!("cargo:rerun-if-changed={script}");
    println!("cargo:rustc-link-arg=-T{script}");
}
