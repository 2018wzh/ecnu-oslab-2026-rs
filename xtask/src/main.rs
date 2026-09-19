//! `xtask` — ECNU OSLab 2026 的主机侧构建工具, 负责构建内核、生成镜像、
//! 启动 QEMU / 打印开发板部署步骤等子命令。

mod build;
mod config;
mod fit;
mod mkfs;
mod run;
mod toolchain;
mod user;

use std::process::ExitCode;

// 命令行入口, 按子命令分发。
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // 没有子命令或第一个参数是 help/-h/--help。
    if args.is_empty() {
        print_usage();
        return ExitCode::SUCCESS;
    }
    match args[0].as_str() {
        "help" | "-h" | "--help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        "list" => cmd_list(&args[1..]),
        "build" => cmd_build(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "image" => cmd_image(&args[1..]),
        "debug" => cmd_debug(&args[1..]),
        "clean" => cmd_clean(&args[1..]),
        "info" => cmd_info(&args[1..]),
        "disk" => cmd_disk(&args[1..]),
        other => {
            eprintln!("xtask: 未知的子命令 {other:?}\n");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

// 打印用法。
fn print_usage() {
    println!(
        r#"ECNU OSLab 2026 (Rust) — 构建工具

用法:
    cargo xtask <子命令> [选项]

子命令:
    list                     列出 configs/ 下所有可用的配置
    info   --config <name>   打印该配置的详细信息 (地址、QEMU 参数)
    build  --config <name>   构建内核
    run    --config <name>   构建并运行 (QEMU), 或打印开发板部署步骤
    image  --config <name>   生成可交付镜像 (开发板: U-Boot FIT)
    debug  --config <name>   以调试模式启动 QEMU (等待 gdb 连接)
    clean  --config <name>   清理该配置的构建产物

选项:
    --config <name>   平台配置名 (对应 configs/<name>.toml)。默认取第一个。
    --release         使用 release profile (默认)
    --debug-build     使用 dev profile (编译更快, 内核更慢更大)
    --verbose         打印实际执行的命令
    --timeout <秒>    运行 N 秒后自动终止 QEMU (用于脚本与 CI;
                      内核本身不会退出, 所以交互使用时请用 Ctrl-A X)
    -h, --help        显示本帮助

例子:
    cargo xtask list
    cargo xtask build --config riscv64-qemu-virt
    cargo xtask run   --config riscv64-qemu-virt
    cargo xtask image --config riscv64-visionfive2

注意:
    * 退出 QEMU: 先按 Ctrl-A, 再按 X
    * 开发板的 run 只打印部署步骤 —— 它不会尝试烧写 SD 卡
"#
    );
}

// ===========================================================================
// 子命令: list
// ===========================================================================
fn cmd_list(args: &[String]) -> ExitCode {
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let configs = match config::load_all() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask: {e}");
            return ExitCode::FAILURE;
        }
    };

    if configs.is_empty() {
        eprintln!("xtask: configs/ 目录下没有任何 .toml 配置");
        return ExitCode::FAILURE;
    }

    println!("可用配置 (configs/*.toml):\n");
    for c in &configs {
        println!(
            "  {:<28} arch={:<8} platform={:<14} boot={}",
            c.name, c.arch, c.platform, c.boot
        );
        if verbose {
            println!("      {}", c.description);
            println!(
                "      kernel_load_addr = {:#x}   target = {}",
                c.kernel_load_addr, c.target()
            );
            if c.is_qemu() {
                println!(
                    "      qemu: -machine {}{} -smp {} -m {} -bios {}",
                    c.qemu.machine,
                    if c.qemu.cpu.is_empty() {
                        String::new()
                    } else {
                        format!(",cpu={}", c.qemu.cpu)
                    },
                    c.qemu.smp,
                    c.qemu.memory,
                    c.qemu.bios
                );
            } else {
                println!(
                    "      uboot: load={:#x} entry={:#x}",
                    c.uboot.load_addr, c.uboot.entry_addr
                );
            }
            println!();
        }
    }
    println!("\n用 `cargo xtask list --verbose` 查看每个配置的详细信息。");
    ExitCode::SUCCESS
}

// ===========================================================================
// 选项解析
// ===========================================================================
// 从参数里取出 `--config <name>`。
fn opt_config(args: &[String]) -> Result<String, String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--config" || a == "-c" {
            return it
                .next()
                .cloned()
                .ok_or_else(|| "--config 后面需要一个配置名".to_string());
        }
        // 支持 --config=name 形式。
        if let Some(v) = a.strip_prefix("--config=") {
            return Ok(v.to_string());
        }
    }
    Err("缺少 --config <name>。用 `cargo xtask list` 查看可用配置。".into())
}

// 解析名字对应的配置。
fn resolve_config(name: &str) -> Result<config::Config, String> {
    match config::load(name) {
        Ok(c) => Ok(c),
        Err(e) => {
            // 拼错配置名很常见, 报错时顺带列出所有合法取值。
            let mut msg = format!("{e}\n\n可用的配置:\n");
            if let Ok(all) = config::load_all() {
                for c in all {
                    msg.push_str(&format!("  {}\n", c.name));
                }
            }
            msg.push_str("\n提示: 配置名里包含 arch-platform-boot 三个维度,\n");
            msg.push_str("      例如 riscv64-qemu-virt 表示 riscv64 + QEMU virt + OpenSBI。");
            Err(msg)
        }
    }
}

// 是否为 release 构建 (默认是)。
fn opt_release(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--debug-build")
}

// 是否打印执行的命令。
fn opt_verbose(args: &[String]) -> bool {
    args.iter().any(|a| a == "--verbose" || a == "-v")
}

// ===========================================================================
// 子命令: build / run / image / debug / clean / info
// ===========================================================================

fn cmd_build(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let verbose = opt_verbose(args);

    // ---- 第 1 步: 先构建用户程序 ----
    // 内核 build.rs 会 include_bytes! 嵌入用户程序映像, 所以用户程序必须先构建好。
    // 【本阶段还没有用户程序】: `user/` 要到 lab-4 才出现。
    // 用"crate 是否存在"来判断, 而不是把这一步删掉 —— 后面的阶段直接
    // 合并即可, 不需要再改回来。
    if build::workspace_root().join("user/Cargo.toml").exists() {
        if let Err(e) = user::build_user_programs(&cfg, verbose) {
            eprintln!("\nxtask: 用户程序构建失败\n{e}");
            return ExitCode::FAILURE;
        }
    }

    match build::build_kernel(&cfg, opt_release(args), verbose) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nxtask: 构建失败\n{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_run(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match run::run_with_options(&cfg, opt_release(args), opt_verbose(args), args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("\nxtask: 运行失败\n{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_image(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match build::make_image(&cfg, opt_release(args), opt_verbose(args)) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nxtask: 生成镜像失败\n{e}");
            ExitCode::FAILURE
        }
    }
}

// `cargo xtask disk` —— 生成磁盘镜像 (含 Rust 用户程序)。
fn cmd_disk(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let verbose = opt_verbose(args);
    match make_disk(&cfg, verbose) {
        Ok(p) => {
            println!("\n磁盘镜像: {}", p.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("\nxtask: 生成磁盘镜像失败\n{e}");
            ExitCode::FAILURE
        }
    }
}

// 生成 `target/disk.img`, 内含所有已构建的用户程序。
fn make_disk(cfg: &crate::config::Config, verbose: bool) -> Result<std::path::PathBuf, String> {
    let ws = build::workspace_root();
    let out = ws.join("target/disk.img");

    // 先构建最新用户程序, 避免打进旧版本。
    let progs = user::build_user_programs(cfg, verbose)?;
    if progs.is_empty() {
        return Err("没有找到任何用户程序 (user/src/bin/*.rs)".into());
    }

    // 镜像内文件名即程序名 (定长 14 字节, 名字要短)。
    // 放 ELF 而非扁平 .bin: 内核用 ELF 加载器装入, 不依赖"文件偏移 == 虚拟地址偏移"。
    let files: Vec<(String, std::path::PathBuf)> = progs
        .iter()
        .map(|p| (p.name.clone(), p.elf_stripped.clone()))
        .collect();

    if verbose {
        for (n, p) in &files {
            println!("  [disk] 加入 {n} <- {}", p.display());
        }
    }

    mkfs::build_image(&out, &files)?;
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!(
        "  [disk] {} ({} 块, {} KB)",
        out.display(),
        size / 512,
        size / 1024
    );
    Ok(out)
}

fn cmd_debug(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match run::debug(&cfg, opt_release(args), opt_verbose(args)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("\nxtask: 调试启动失败\n{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_clean(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match build::clean(&cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nxtask: 清理失败\n{e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_info(args: &[String]) -> ExitCode {
    let cfg = match parse_common(args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    print_config_info(&cfg);
    println!();
    println!("---- 构建工具 (rustup/llvm-tools) ----");
    println!("{}", toolchain::describe());
    ExitCode::SUCCESS
}

// 解析 build/run/image/debug/clean/info 共用的 `--config` 选项。
fn parse_common(args: &[String]) -> Result<config::Config, ExitCode> {
    let name = match opt_config(args) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("xtask: {e}");
            return Err(ExitCode::FAILURE);
        }
    };
    resolve_config(&name).map_err(|e| {
        eprintln!("xtask: {e}");
        ExitCode::FAILURE
    })
}

// 打印一个配置的详细信息。不用设备树, 地址是编译期常量,
// 需要能方便打印以人工核对。
fn print_config_info(c: &config::Config) {
    println!("================ 配置详情 ================");
    println!("  配置名      : {}", c.name);
    println!("  描述        : {}", c.description);
    println!();
    println!("  ---- 三个正交的维度 ----");
    println!("  arch        : {}   (CPU/ISA 语义)", c.arch);
    println!("  platform    : {}   (机器语义)", c.platform);
    println!("  boot        : {}   (启动路径语义)", c.boot);
    println!();
    println!("  ---- 构建 ----");
    println!("  target      : {}", c.target());
    println!("  链接地址    : {:#x}", c.kernel_load_addr);
    println!("  产物目录    : {}", build::artifact_dir(c).display());
    println!();
    if c.is_qemu() {
        println!("  ---- QEMU ----");
        println!("  -machine    : {}", c.qemu.machine);
        println!("  -cpu        : {}", c.qemu.cpu);
        println!("  -smp        : {}", c.qemu.smp);
        println!("  -m          : {}", c.qemu.memory);
        println!("  -bios       : {}", c.qemu.bios);
        println!();
        println!("  运行: cargo xtask run --config {}", c.name);
    } else {
        println!("  ---- U-Boot ----");
        println!("  load addr   : {:#x}", c.uboot.load_addr);
        println!("  entry addr  : {:#x}", c.uboot.entry_addr);
        println!("  boot 命令   : {}", c.uboot.boot_command);
        println!();
        println!("  生成镜像: cargo xtask image --config {}", c.name);
        println!("  部署步骤:");
        for line in c.uboot.deploy_hint.lines() {
            println!("    {line}");
        }
    }
    println!("==========================================");
}
