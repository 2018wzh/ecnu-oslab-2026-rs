//! `toolchain` — 从 rustup 管理的工具链里定位编译/链接工具。
//!
//! ## 为什么需要这一层
//!
//! 本仓库**不依赖系统安装的交叉工具链** (`riscv64-elf-ld` / `riscv64-elf-objcopy`
//! 等)。构建需要的链接器与 binutils 全部来自 Rust 工具链自己:
//!
//! * 链接器 `rust-lld` (随工具链自带);
//! * `llvm-objcopy` / `llvm-nm` / `llvm-objdump` (来自 `rustup component
//!   add llvm-tools`)。
//!
//! 这些可执行文件位于
//! `<sysroot>/lib/rustlib/<host-triple>/bin/`, 而 `<host-triple>` 与
//! `<sysroot>` 都要在运行时向 rustc 查询 —— 它们随安装方式、工具链版本、
//! 平台而异, **不能写死**。
//!
//! ## 为什么用运行中的那个 rustc
//!
//! 本仓库用 `rust-toolchain.toml` 钉住 channel (`1.97.1`)。`cargo` /
//! `rustc` 通过 rustup 的 shim 解析到那个工具链; 当多个工具链共存时,
//! PATH 上可能有别的 rustc。所以这里用 `CARGO` 环境变量(xtask 由
//! `cargo xtask` 启动, cargo 会设它)定位"启动我的那个 cargo", 取它的
//! 兄弟 rustc —— 保证用的是**同一个**工具链。

use std::path::PathBuf;
use std::process::Command;

/// cargo 可执行文件(与 build.rs 的 `cargo_binary` 同源, 但这里只用来
/// 推导 rustc 的位置)。
fn cargo_binary() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// 与当前 cargo 同一工具链的 rustc。
fn rustc_binary() -> String {
    // CARGO 指向 `.../bin/cargo`, 它的兄弟 `rustc` 就是同一工具链的 rustc。
    // 没有 CARGO (例如直接跑 xtask) 时退回 PATH 上的 `rustc`。
    let cargo = cargo_binary();
    let p = PathBuf::from(&cargo);
    let sibling = p.with_file_name("rustc");
    if sibling.exists() {
        sibling.to_string_lossy().into_owned()
    } else {
        "rustc".into()
    }
}

/// rustup 工具链里的工具目录:
/// `<sysroot>/lib/rustlib/<host>/bin/`, 其中放 `rust-lld` 与 `llvm-*`。
pub fn tools_dir() -> Result<PathBuf, String> {
    let rustc = rustc_binary();
    let sysroot = run_capture(&rustc, &["--print", "sysroot"])?;
    let host = run_capture(&rustc, &["-vV"])?
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or_else(|| "无法从 rustc -vV 解析 host 三元组".to_string())?
        .to_string();

    let dir = PathBuf::from(sysroot.trim())
        .join("lib/rustlib")
        .join(host)
        .join("bin");
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(format!(
            "找不到 rustup 工具目录 {}。\n\
             请确认: (1) 当前使用的是 rustup 管理的工具链;\n\
             (2) 已安装 llvm-tools 组件 (`rustup component add llvm-tools`)。",
            dir.display()
        ))
    }
}

/// 定位一个工具, **只**从 rustup 工具链自带的工具里找, 不回退到 PATH。
///
/// rustup 的 llvm-tools 只提供架构无关的 `llvm-*` 名 (`llvm-objcopy` /
/// `llvm-nm` / `llvm-objdump`); 链接器是 `rust-lld` (叫 `ld` 时映射过去)。
pub fn find(name: &str) -> Option<PathBuf> {
    let dir = tools_dir().ok()?;
    let p = dir.join(format!("llvm-{name}"));
    if p.exists() {
        return Some(p);
    }
    // binutils 别名 (ld 需要的是 rust-lld, 不叫 llvm-ld)。
    let alias = dir.join(if name == "ld" { "rust-lld" } else { name });
    if alias.exists() {
        return Some(alias);
    }
    None
}

/// 把 rustup 工具目录加到 PATH 开头, 并补上工具链的动态库目录
/// (llvm 工具需要)。返回准备给子进程的环境修改变量。
///
/// 返回 `(允许用哪种方式修 PATH, 需要用到的目录 vec)` —— 具体拼接
/// 由调用方决定, 这里只给出信息。
pub fn path_setup() -> (PathBuf, PathBuf) {
    // 默认: 目录本身 + 其父 lib 目录(放 libLLVM)。稳妥起见两者都加。
    match tools_dir() {
        Ok(dir) => {
            // libLLVM 在 <sysroot>/lib 与 rustlib/<host>/lib。
            let sysroot = dir
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .and_then(|p| p.parent()); // .../lib/rustlib/<host>/bin -> .../lib
            let lib = sysroot.map(|p| p.join("lib"));
            (dir, lib.unwrap_or_default())
        }
        Err(_) => (PathBuf::new(), PathBuf::new()),
    }
}

/// 把工具目录与 lib 目录注入到 (PATH, LD_LIBRARY_PATH), 供子进程使用。
pub fn env_for_child() -> Vec<(String, String)> {
    let (dir, lib) = path_setup();
    let mut out = Vec::new();
    let path = std::env::var("PATH").unwrap_or_default();
    if !dir.as_os_str().is_empty() {
        let new_path = format!("{}:{}", dir.display(), path);
        out.push(("PATH".into(), new_path));
    }
    if !lib.as_os_str().is_empty() {
        let old = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let new_ld = if old.is_empty() {
            lib.display().to_string()
        } else {
            format!("{}:{}", lib.display(), old)
        };
        out.push(("LD_LIBRARY_PATH".into(), new_ld));
    }
    out
}

/// 运行命令并捕获 stdout (按字符串返回首行尾部)。
fn run_capture(prog: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| format!("无法执行 {}: {e}", prog))?;
    if !out.status.success() {
        return Err(format!("{} 报告了错误", prog));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 打印可用的构建工具(调试用)。
pub fn describe() -> String {
    match tools_dir() {
        Ok(dir) => {
            let linker = dir.join("rust-lld");
            let has_ld = linker.exists();
            let has_objcopy = dir.join("llvm-objcopy").exists();
            format!(
                "工具目录: {}\n  rust-lld(链接器): {}\n  llvm-objcopy: {}\n  llvm-nm/objdump: {}",
                dir.display(),
                if has_ld { "有" } else { "无" },
                if has_objcopy { "有" } else { "无" },
                if dir.join("llvm-nm").exists() && dir.join("llvm-objdump").exists() {
                    "有"
                } else {
                    "无"
                }
            )
        }
        Err(e) => e,
    }
}