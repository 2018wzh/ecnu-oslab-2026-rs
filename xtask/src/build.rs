//! `xtask::build` — 调用 cargo 构建内核, 并生成可交付镜像。
//! 决定目标与 feature、注入链接地址、校验 `_entry` 地址、生成 FIT。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

// 仓库根目录。
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("xtask 应该在仓库根目录的子目录里")
        .to_path_buf()
}

// 某个配置的产物目录。
//
// 不同配置的产物不能互相覆盖: QEMU 是 0x80200000 起的 ELF, VF2 是
// 0x40200000 起的 FIT, 混在一起会让人困惑。用配置名而非 platform 名命名。
pub fn artifact_dir(cfg: &Config) -> PathBuf {
    workspace_root()
        .join("target")
        .join(cfg.artifact_dir_name())
}

// cargo 的 target 目录。
fn cargo_target_dir() -> PathBuf {
    // 尊重 CARGO_TARGET_DIR (cargo 的标准环境变量)。
    if let Ok(d) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(d);
    }
    workspace_root().join("target")
}

// 内核 ELF 的路径 (cargo 默认布局)。
fn kernel_elf_path(cfg: &Config, release: bool) -> PathBuf {
    let profile = if release { "release" } else { "debug" };
    cargo_target_dir()
        .join(cfg.target())
        .join(profile)
        .join("oslab-kernel")
}

// 编译内核, 返回 ELF 的路径。
pub fn build_kernel(cfg: &Config, release: bool, verbose: bool) -> Result<PathBuf, String> {
    println!("==> 构建内核");
    println!("    配置        : {}", cfg.name);
    println!("    arch        : {}", cfg.arch);
    println!("    platform    : {}", cfg.platform);
    println!("    boot        : {}", cfg.boot);
    println!("    target      : {}", cfg.target());
    println!("    链接地址    : {:#x}", cfg.kernel_load_addr);
    // 把链接脚本也打出来: 它是配置项而不是写死的路径, 值应在用的时候可见。
    println!("    链接脚本    : {}", cfg.linker);
    println!("    features    : {}", cfg.cargo_features());
    println!();

    // 产物目录先建好, 因为 OSLAB_CONFIG 与镜像输出都在该目录下。
    let art = artifact_dir(cfg);
    std::fs::create_dir_all(&art).map_err(|e| format!("无法创建 {}: {e}", art.display()))?;

    // 配置文件路径: kernel 的 build.rs 需要它来生成链接脚本。
    let cfg_path = workspace_root()
        .join("configs")
        .join(format!("{}.toml", cfg.name));
    if !cfg_path.exists() {
        return Err(format!("找不到配置文件 {}", cfg_path.display()));
    }

    let mut cmd = Command::new(cargo_binary());
    cmd.current_dir(workspace_root());
    cmd.arg("build");
    cmd.arg("--target").arg(cfg.target());
    cmd.arg("-p").arg("oslab-kernel");
    cmd.arg("--features").arg(cfg.cargo_features());
    if release {
        cmd.arg("--release");
    }
    // 关闭增量编译: 增量编译有时让"改了 build.rs 但链接脚本没更新"这类问题出现。
    cmd.env("CARGO_INCREMENTAL", "0");
    // build.rs 需要知道该注入哪个地址。
    cmd.env("OSLAB_CONFIG", &cfg_path);
    // build.rs 退化时按 <arch>-<platform>.toml 找配置。
    cmd.env("OSLAB_PLATFORM", &cfg.platform);
    cmd.env("OSLAB_ARCH", &cfg.arch);

    // 没有外部依赖, 永远离线, 避免无网络时的索引刷新。
    cmd.env("CARGO_NET_OFFLINE", "true");
    // 把 rustup 的 llvm-tools 目录注入 PATH (以及动态库目录),
    // 让 `.cargo/config.toml` 里 `linker = "rust-lld"` 能被解析到。
    for (k, v) in crate::toolchain::env_for_child() {
        cmd.env(k, v);
    }

    let status = run_command(&mut cmd, verbose)?;
    if !status {
        return Err("cargo build 失败 (见上面的输出)".into());
    }

    let elf = kernel_elf_path(cfg, release);
    if !elf.exists() {
        return Err(format!(
            "构建成功但找不到产物 {}\n\
             这通常说明 [[bin]] 的名字与 build.rs 里的假设不一致。",
            elf.display()
        ));
    }

    // ---- 校验链接地址 ----
    verify_link_address(cfg, &elf, verbose)?;

    // ---- 复制到产物目录, 让学生的路径稳定 ----
    let staged = art.join("kernel.elf");
    std::fs::copy(&elf, &staged)
        .map_err(|e| format!("无法复制 {} -> {}: {e}", elf.display(), staged.display()))?;

    // 同时生成反汇编与符号表, 排查启动问题时最有用。
    dump_auxiliary(&elf, &art, verbose)?;

    let size = std::fs::metadata(&elf).map(|m| m.len()).unwrap_or(0);
    println!("\n==> 构建完成");
    println!("    ELF      : {}", staged.display());
    println!("    大小     : {} 字节", size);
    Ok(staged)
}

// 用 `nm` 校验 `_entry` 地址, 确保"链接地址 == 加载地址"。
fn verify_link_address(cfg: &Config, elf: &Path, verbose: bool) -> Result<(), String> {
    let nm = find_tool("nm")?;

    let out = Command::new(&nm)
        .arg(elf)
        .output()
        .map_err(|e| format!("执行 {} 失败: {e}", nm))?;
    if !out.status.success() {
        return Err(format!("{} 报告了错误", nm));
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // nm 的输出形如: `0000000080200000 T _entry`
    let mut entry: Option<u64> = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(addr), Some(_ty), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if name == cfg.facts.entry_symbol {
            entry = u64::from_str_radix(addr, 16).ok();
            break;
        }
    }

    let entry = entry.ok_or_else(|| {
        format!(
            "在 {} 里找不到符号 {}。\n\
             这通常说明入口汇编没有被链接进来 —— 检查 \
             crates/hal/src/arch/<arch>/boot.rs 里的 global_asm! 是否还在。",
            elf.display(),
            cfg.facts.entry_symbol
        )
    })?;

    if verbose {
        println!("    nm: _entry = {entry:#x}");
    }

    if entry != cfg.kernel_load_addr {
        return Err(format!(
            "\n\
             ============================================================\n\
             错误: 链接地址与配置不符\n\
             \n\
             {} 的实际地址 : {entry:#x}\n\
             配置声明的地址    : {:#x}\n\
             \n\
             这说明 crates/kernel/build.rs 生成的链接脚本没有被真正用上,\n\
             或者 platforms 层的 kernel_base 与配置里的 kernel_load_addr\n\
             不一致。请检查:\n\
               * configs/{}.toml 的 kernel_load_addr\n\
               * crates/hal/src/platform/*.rs 的 kernel_base\n\
               * crates/kernel/build.rs 是否把 -T<脚本> 传给了链接器\n\
             ============================================================\n",
            cfg.facts.entry_symbol, cfg.kernel_load_addr, cfg.name
        ));
    }

    println!(
        "    [ok] {} = {entry:#x} (与配置一致)",
        cfg.facts.entry_symbol
    );
    Ok(())
}

// 生成反汇编与符号表, 排查启动问题时默认就备好。
fn dump_auxiliary(elf: &Path, art: &Path, verbose: bool) -> Result<(), String> {
    let objdump = find_tool("objdump")?;
    let nm = find_tool("nm")?;

    let out = art.join("kernel.asm");
    // 输出可能有几 MB, 用 shell 重定向而非读到内存再写盘。
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} -d {} > {}",
            shell_quote(&objdump),
            shell_quote(elf.to_str().unwrap_or("")),
            shell_quote(out.to_str().unwrap_or(""))
        ))
        .status();
    if let Ok(s) = status {
        if s.success() && verbose {
            println!("    反汇编   : {}", out.display());
        }
    }

    let out = art.join("kernel.sym");
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} -n {} > {}",
            shell_quote(&nm),
            shell_quote(elf.to_str().unwrap_or("")),
            shell_quote(out.to_str().unwrap_or(""))
        ))
        .status();
    if let Ok(s) = status {
        if s.success() && verbose {
            println!("    符号表   : {}", out.display());
        }
    }
    Ok(())
}

// 生成可交付镜像: QEMU 直接复 ELF; 开发板生成 U-Boot FIT。
pub fn make_image(cfg: &Config, release: bool, verbose: bool) -> Result<PathBuf, String> {
    let elf = build_kernel(cfg, release, verbose)?;
    let art = artifact_dir(cfg);

    if cfg.is_qemu() {
        // QEMU 能直接读 ELF 的段表, 所以"镜像"就是 ELF。
        println!("\n==> QEMU 镜像已就绪");
        println!("    {}", elf.display());
        println!("    运行: cargo xtask run --config {}", cfg.name);
        return Ok(elf);
    }

    // ---- 开发板: 生成 FIT ----
    // FIT 放裸二进制而非 ELF: 加载/入口地址由 FIT 属性声明, 且体积更小。
    let bin = objcopy_to_bin(&elf, &art, verbose)?;

    let itb = art.join("kernel.itb");
    // description 会进 U-Boot 的控制台输出, 必须是纯 ASCII —— 多字节字符
    // 会被 `dtc` 当成非打印字节、退化成十六进制数组。
    let desc = format!("ECNU OSLab 2026 (Rust) - {}", cfg.name);
    crate::fit::generate(
        &bin,
        &itb,
        cfg.uboot.load_addr,
        cfg.uboot.entry_addr,
        &cfg.facts.fit_arch,
        &desc,
    )?;

    println!("\n==> 开发板交付物已生成");
    println!("    FIT 镜像 : {}", itb.display());
    println!("    裸二进制 : {}", bin.display());
    println!();
    println!("    部署步骤:");
    for line in cfg.uboot.deploy_hint.lines() {
        if !line.trim().is_empty() {
            println!("    {line}");
        }
    }
    Ok(itb)
}

// 用 objcopy 生成裸二进制。
fn objcopy_to_bin(elf: &Path, art: &Path, verbose: bool) -> Result<PathBuf, String> {
    let objcopy = find_tool("objcopy")?;
    let bin = art.join("kernel.bin");
    let mut cmd = Command::new(&objcopy);
    // -O binary 输出裸二进制, 不含 .bss (无文件内容), 故内核启动时须自己清 .bss。
    cmd.arg("-O").arg("binary").arg(elf).arg(&bin);
    if !run_command(&mut cmd, verbose)? {
        return Err("objcopy 失败".into());
    }
    Ok(bin)
}

// 清理某个配置的产物。
pub fn clean(cfg: &Config) -> Result<(), String> {
    let art = artifact_dir(cfg);
    if art.exists() {
        std::fs::remove_dir_all(&art).map_err(|e| format!("无法删除 {}: {e}", art.display()))?;
        println!("已删除 {}", art.display());
    }
    // 清掉 cargo 产物, 避免"链接脚本更新了但目标文件是旧的"。
    let mut cmd = Command::new(cargo_binary());
    cmd.current_dir(workspace_root());
    cmd.args(["clean", "-p", "oslab-kernel"]);
    let _ = run_command(&mut cmd, true);
    Ok(())
}

// ===========================================================================
// 工具函数
// ===========================================================================

// cargo 可执行文件的名字。
//
// 用 `CARGO` 环境变量, cargo 会指向启动自己的那个 cargo, 避免多工具链下版本不一致。
fn cargo_binary() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

// 找一个工具 (`objdump` / `nm` / `objcopy`), **只**从 rustup 的 llvm-tools
// 里找 (`toolchain::find`), 找不到就报错 —— 本仓库构建需要 rustup 的
// llvm-tools 组件, 不依赖系统安装的任何 binutils。
fn find_tool(name: &str) -> Result<String, String> {
    let Some(p) = crate::toolchain::find(name) else {
        return Err(format!(
            "找不到 llvm-{name}。请安装 rustup 的 llvm-tools 组件: \
             rustup component add llvm-tools"
        ));
    };
    Ok(p.to_string_lossy().into_owned())
}

// 执行一个命令, 打印它, 返回是否成功。
pub fn run_command(cmd: &mut Command, verbose: bool) -> Result<bool, String> {
    if verbose {
        println!("    $ {}", format_command(cmd));
    }
    let status = cmd
        .status()
        .map_err(|e| format!("无法执行 {}: {e}", format_command(cmd)))?;
    Ok(status.success())
}

// 把一个 `Command` 渲染成可粘贴到 shell 的一行, 失败时学生可直接重复调试。
fn format_command(cmd: &Command) -> String {
    let mut s = String::new();
    s.push_str(&shell_quote(&cmd.get_program().to_string_lossy()));
    for a in cmd.get_args() {
        s.push(' ');
        s.push_str(&shell_quote(&a.to_string_lossy()));
    }
    // 把设置的环境变量也打印出来 —— 它们常常是问题所在。
    for (k, v) in cmd.get_envs() {
        if let Some(v) = v {
            s = format!(
                "{}={} {}",
                k.to_string_lossy(),
                shell_quote(&v.to_string_lossy()),
                s
            );
        }
    }
    s
}

// 给字符串加 shell 引号 (只在必要时)。
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    let needs = s
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || "._-+=/:@".contains(c)));
    if needs {
        format!("'{}'", s.replace('\'', r"'\''"))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("plain"), "plain");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("${kernel_addr_r}"), "'${kernel_addr_r}'");
    }
}
