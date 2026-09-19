//! `user` 包的构建脚本 —— 把用户程序的链接选项注册给 cargo。
//!
//! ## 它做什么
//!
//! 用户程序是裸机 `no_std` 二进制, 需要:
//!
//!   1. 用**我们自己的链接脚本** (user/arch/<arch>/user.ld.in 生成的成品)
//!      而不是 rustc 的默认脚本 —— 它指定基址 0x1000 与入口 `_user_start`;
//!   2. 设置 `--image-base` (LLD 才有的选项) 与页对齐;
//!   3. 强制**单个** RWX LOAD 段 (`-N` / `--omagic`), 与旧 GNU ld 的产出
//!      一致: LLD 默认会把 .text(R X) 与 .rodata(R) / .data(R W) 拆成多个
//!      LOAD 段, 而这两个段的 vaddr 落在同一页内时, 内核的 ELF 加载器
//!      (crates/kernel/src/proc/elf.rs) 按"每段重映射到新页"会发生第二次
//!      映射覆盖第一次的可执行页 —— 表现为执行入口指令缺页。
//!
//! 这些选项通过 `cargo::rustc-link-arg-bins=` 只作用于**本包的所有 bin**
//! (`src/bin/*.rs`), 不会带进内核。脚本路径、基址、页大小由 xtask
//! 通过环境变量传入 (见 xtask/src/user.rs 的 build_user_programs), 因为
//! 只有 xtask 在运行时才知道架构配置。

fn main() {
    // 链接脚本 (绝对路径, 通常由 xtask 生成)。
    //
    // 没有 OSLAB_USER_LD 时**不 panic**: 那会让 `cargo check` / `cargo test`
    // (rust-analyzer 也会) 在没有 xtask 上下文时直接失败。这里退化成"不注册
    // link-arg"并给出一条可指引去的警告 —— 真正的链接必须走 xtask
    // (后者会设置环境变量再调 cargo build), 缺它时 bin 会因缺脚本/入口而
    // 在链接期失败, 那个错误信息更清晰。
    let Ok(ld) = std::env::var("OSLAB_USER_LD") else {
        eprintln!(
            "cargo:warning=oslab-user: 缺少 OSLAB_USER_LD, 跳过链接脚本注册。\n\
             cargo:warning=  正常构建请用 `cargo xtask build --config <name>`。\n\
             cargo:warning=  (本警告在 IDE / `cargo check` 时是可预期的)"
        );
        return;
    };
    let base = std::env::var("OSLAB_USER_BASE").unwrap_or_else(|_| "0x1000".into());
    let page = std::env::var("OSLAB_PAGE").unwrap_or_else(|_| "4096".into());

    // 这些变量变化时重跑本脚本 (换平台/改模板都要重新注册链接选项)。
    println!("cargo::rerun-if-env-changed=OSLAB_USER_LD");
    println!("cargo::rerun-if-env-changed=OSLAB_USER_BASE");
    println!("cargo::rerun-if-env-changed=OSLAB_PAGE");
    // 链接脚本本身变了也要重跑。
    println!("cargo::rerun-if-changed={ld}");

    // 注册到本包的所有二进制。
    println!("cargo::rustc-link-arg-bins=-T{ld}");
    println!("cargo::rustc-link-arg-bins=--image-base={base}");
    println!("cargo::rustc-link-arg-bins=-z");
    println!("cargo::rustc-link-arg-bins=max-page-size={page}");
    // 强制**单个** RWX LOAD 段, 与旧 GNU ld 产出一致, 避免多段共享页
    // 时 ELF 加载器 (crates/kernel/src/proc/elf.rs) 的按段重映射覆盖
    // 掉可执行页 (即"指令缺页" bug)。见 build.rs 顶部注释。
    println!("cargo::rustc-link-arg-bins=-N");
}