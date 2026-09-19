//! `user` — 构建用户态程序。
//!
//! ## 构建方式
//!
//! 用户程序是真正独立的二进制, 依赖 `oslab-user` 运行时 crate。构建分两步:
//!
//! ```text
//!   cargo build -p oslab-user --target <triple> --release
//!      -> 每个 src/bin/<name>.rs 链接成 ELF (链接器 script 由
//!         user/build.rs 从 OSLAB_USER_LD 等环境变量读出后注册)
//!   llvm-objcopy / llvm-nm
//!      -> 导出扁平 .bin (内核按字节装入) 与 stripped .elf (磁盘装载)
//! ```
//!
//! 架构相关的三元组、链接脚本模板、装入基址均来自配置
//! (`configs/arch/<arch>.toml`), 本模块不认识任何具体架构。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::workspace_root;
use crate::config::Config;
use crate::toolchain;

// 一个用户程序的构建产物。
pub struct UserProgram {
    // 程序名 (来自 `user/src/bin/<name>.rs`)。
    pub name: String,
    // 去掉调试信息的 ELF —— 从磁盘装入时用的就是它。
    // 带调试信息的 .elf 与扁平 .bin 留在 target/user/, 但不出现在这里。
    pub elf_stripped: PathBuf,
}

// 构建 `user/src/bin/` 下的所有用户程序。
pub fn build_user_programs(cfg: &Config, verbose: bool) -> Result<Vec<UserProgram>, String> {
    let ws = workspace_root();
    let bin_dir = ws.join("user/src/bin");
    let out_dir = ws.join("target/user");

    if !bin_dir.is_dir() {
        // 早期 lab 分支可能还没有用户程序, 不是错误。
        return Ok(Vec::new());
    }
    let has_bins = std::fs::read_dir(&bin_dir)
        .map_err(|e| format!("无法读取 {}: {e}", bin_dir.display()))?
        .next()
        .is_some();
    if !has_bins {
        return Ok(Vec::new());
    }

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("无法创建 {}: {e}", out_dir.display()))?;

    // ---- 1. 生成链接脚本 (供 user/build.rs 用) ----
    let linker = generate_user_linker(cfg, &out_dir)?;

    // ---- 2. 用 cargo 编译 + 链接所有用户程序 ----
    // 编译器/链接器都在 cargo 内部完成; 链接脚本、基址、页大小通过
    // 环境变量交给 user/build.rs, 它再 `cargo::rustc-link-arg-bins` 注册
    // 到本包的所有 bin。这样链接选项只作用于用户程序, 不影响内核。
    let mut cmd = Command::new(cargo_binary());
    cmd.current_dir(&ws);
    cmd.arg("build");
    cmd.arg("-p").arg("oslab-user");
    cmd.arg("--target").arg(cfg.facts.target.as_str());
    // 架构选择 (系统调用机制), 与内核 hal 的 arch-<arch> 同一约定。
    cmd.arg("--features").arg(format!("arch-{}", cfg.arch));
    cmd.arg("--release");
    cmd.env("OSLAB_USER_LD", &linker);
    cmd.env("OSLAB_USER_BASE", &cfg.facts.user_base.to_string());
    cmd.env("OSLAB_PAGE", &cfg.facts.page_size.to_string());
    cmd.env("CARGO_NET_OFFLINE", "true");
    // 让 `linker = "rust-lld"` (在 .cargo/config.toml) 能被解析到。
    for (k, v) in toolchain::env_for_child() {
        cmd.env(k, v);
    }

    let status = cmd
        .status()
        .map_err(|e| format!("无法执行 cargo 构建用户程序: {e}"))?;
    if !status.success() {
        return Err("cargo 构建用户程序失败 (见上方输出)".into());
    }

    // ---- 3. 取出所有程序名 ----
    let mut names: Vec<String> = std::fs::read_dir(&bin_dir)
        .map_err(|e| format!("无法读取 {}: {e}", bin_dir.display()))?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                p.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();

    // ---- 4. 从 cargo 产物导出 .bin / .elf ----
    // 尊重 CARGO_TARGET_DIR (与 build.rs 的 cargo_target_dir 一致)。
    let cargo_root = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| ws.join("target"));
    let cargo_bin_dir = cargo_root
        .join(cfg.facts.target.as_str())
        .join("release");

    let mut out = Vec::new();
    for name in names {
        // cargo 把每个 bin 链接成 `target/<target>/release/<name>`。
        let elf_cargo = cargo_bin_dir.join(&name);
        if !elf_cargo.exists() {
            return Err(format!(
                "cargo 没有产出用户程序 ELF {} —— 检查 user/src/bin/{name}.rs 是否编译通过",
                elf_cargo.display()
            ));
        }
        let elf = out_dir.join(format!("{name}.elf"));
        let bin = out_dir.join(format!("{name}.bin"));
        let elf_stripped = out_dir.join(format!("{name}.stripped.elf"));

        if verbose {
            println!("  [user] 编译 {name}");
        }

        // ---- 4a. 复制带调试信息的 ELF ----
        std::fs::copy(&elf_cargo, &elf)
            .map_err(|e| format!("无法复制 {} -> {}: {e}", elf_cargo.display(), elf.display()))?;

        // ---- 4b. 读取入口地址 ----
        let entry = read_entry(&elf, &name)?;

        // ---- 4c. 扁平化为 .bin ----
        // strip 调调试段: 映像大好几倍, 内核逐字节装入, 那些段既没用又占空间。
        let objcopy = find_tool("objcopy")?;
        run(&objcopy, &["-O", "binary", "--strip-all"], &elf, &bin, "objcopy")?;

        // ---- 4d. 去掉调试信息的 ELF (磁盘上用的就是它) ----
        // 必须用 --strip-debug 而非 --strip-all: 后者连符号表一起去掉,
        // objcopy 会重排节, `_user_start` 不再是 .text 第一块, 内核算照
        // 旧 e_entry 跳到别的函数, 程序从错误入口跑起来即"卡住"。
        run(&objcopy, &["--strip-debug"], &elf, &elf_stripped, "strip")?;

        let size = std::fs::metadata(&bin).map(|m| m.len()).unwrap_or(0);
        let esize = std::fs::metadata(&elf_stripped).map(|m| m.len()).unwrap_or(0);
        println!("  [user] {name:<8} entry=0x{entry:x}  flat={size}  elf={esize} bytes");

        out.push(UserProgram {
            name,
            elf_stripped,
        });
    }

    Ok(out)
}

// 读取 ELF 里 `_user_start` 的地址 (用 `nm`, 宿主工具无需自己解析 ELF)。
fn read_entry(elf: &Path, name: &str) -> Result<u64, String> {
    let nm = find_tool("nm")?;
    let out = Command::new(&nm)
        .arg(elf)
        .output()
        .map_err(|e| format!("无法执行 {}: {e}", nm))?;
    let text = String::from_utf8_lossy(&out.stdout);

    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[2] == "_user_start" {
            return u64::from_str_radix(parts[0], 16)
                .map_err(|e| format!("无法解析 {name} 的 _user_start 地址: {e}"));
        }
    }
    Err(format!(
        "{name} 里找不到 _user_start —— 是不是忘了写 `entry!(main);`?"
    ))
}

// 生成用户程序的链接脚本: 从配置指定的模板 (user/arch/<arch>/user.ld.in)
// 填好架构名与基址, 成品写 target/user/user.ld, 不提交进仓库。
fn generate_user_linker(cfg: &Config, out_dir: &Path) -> Result<PathBuf, String> {
    let ws = workspace_root();
    let template_path = ws.join(&cfg.facts.user_linker);
    let template = std::fs::read_to_string(&template_path)
        .map_err(|e| format!("无法读取链接脚本模板 {}: {e}", template_path.display()))?;

    let script = template
        .replace("@ARCH_NAME@", &cfg.facts.ld_arch)
        .replace("@USER_BASE@", &format!("{:#x}", cfg.facts.user_base));

    // 替换后不应还有 @...@ 占位符; 有则说明模板加了新占位符而这里没跟上。
    if script.contains('@') {
        return Err(format!(
            "链接脚本模板 {} 里还有未替换的 @...@ 占位符",
            template_path.display()
        ));
    }

    let out = out_dir.join("user.ld");
    std::fs::write(&out, script).map_err(|e| format!("无法写出 {}: {e}", out.display()))?;
    Ok(out)
}

// 找一个工具 (objcopy / nm / objdump), 只从 rustup 的 llvm-tools 里找。
fn find_tool(name: &str) -> Result<String, String> {
    let Some(p) = toolchain::find(name) else {
        return Err(format!(
            "找不到 llvm-{name}。请安装 rustup 的 llvm-tools 组件: \
             rustup component add llvm-tools"
        ));
    };
    Ok(p.to_string_lossy().into_owned())
}

// 运行一个 objcopy 子命令式调用, 输出到固定文件。
fn run(tool: &str, args: &[&str], input: &Path, output: &Path, what: &str) -> Result<(), String> {
    let status = Command::new(tool)
        .args(args)
        .arg(input)
        .arg(output)
        .status()
        .map_err(|e| format!("无法执行 {tool}: {e}"))?;
    if !status.success() {
        return Err(format!("{what} {input:?} 失败"));
    }
    Ok(())
}

// cargo 可执行文件的名字 (用 CARGO 环境变量保证与启动 xtask 的同一个)。
fn cargo_binary() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}