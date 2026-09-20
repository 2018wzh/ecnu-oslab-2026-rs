fn main() {
    println!("cargo:rerun-if-env-changed=OSLAB_LAB8_TEST");
    let test = std::env::var("OSLAB_LAB8_TEST").unwrap_or("0".into());
    assert!(["0", "1", "2", "3", "4"].contains(&test.as_str()));
    println!("cargo:rustc-env=OSLAB_LAB8_TEST={test}");
    // 显式选择例程时保留链接入口，即使前序学生 TODO 尚未实现。
    if test != "0" { println!("cargo:rustc-link-arg=--undefined=lab8_examples"); }
    println!("cargo:rerun-if-env-changed=OSLAB_LINKER");
    println!("cargo:rerun-if-env-changed=OSLAB_USER_IMAGE");
    let image = std::env::var("OSLAB_USER_IMAGE").expect("请通过 xtask 构建用户镜像");
    println!("cargo:rerun-if-changed={image}");
    println!("cargo:rustc-env=OSLAB_USER_IMAGE={image}");
    let script = std::env::var("OSLAB_LINKER").expect("请使用 cargo xtask build");
    println!("cargo:rerun-if-changed={script}");
    println!("cargo:rustc-link-arg=-T{script}");
}
