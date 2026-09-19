//! `xtask::run` — 在 QEMU 里运行内核, 或打印开发板部署步骤。
//! 开发板不自动烧卡 (危险操作需人工确认), 只打印步骤。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::build;
use crate::config::Config;

// QEMU 的可执行文件名。
//
// 默认取配置的 `qemu_system` (架构事实), 可用环境变量 `QEMU` 覆盖 (发行版包名可能不同)。
fn qemu_binary(cfg: &Config) -> String {
    std::env::var("QEMU").unwrap_or_else(|_| cfg.facts.qemu_system.clone())
}

// 组装 QEMU 参数, 供 run 和 debug 共用, 且可打印出来手工调试。
fn qemu_args(
    cfg: &Config,
    elf: &std::path::Path,
    debug: bool,
    gdb_port: u16,
) -> Result<Vec<String>, String> {
    // 磁盘镜像每次运行都拷一份副本, 否则一次运行会把它改脏, 下一次看到旧内容。
    let disk = crate::build::workspace_root().join("target/disk.img");
    let run_disk = crate::build::workspace_root().join("target/disk-run.img");
    if disk.exists() {
        std::fs::copy(&disk, &run_disk)
            .map_err(|e| format!("无法复制 {} -> {}: {e}", disk.display(), run_disk.display()))?;
    }
    let mut args: Vec<String> = Vec::new();

    // ---- 机器 ----
    // CPU 型号是 `-cpu` 的选项, 不能写成 machine 的属性, 否则 QEMU
    // 报 "Property 'virt-machine.cpu' not found" 且不提示改用 -cpu。
    args.push("-machine".into());
    args.push(cfg.qemu.machine.clone());
    if !cfg.qemu.cpu.is_empty() {
        args.push("-cpu".into());
        args.push(cfg.qemu.cpu.clone());
    }

    // ---- 内存与 CPU 数 ----
    // 必须与 platform 层的 dram_size / ncpu 一致, 由构建期检查保证。
    args.push("-m".into());
    args.push(cfg.qemu.memory.clone());
    args.push("-smp".into());
    args.push(cfg.qemu.smp.to_string());

    // ---- 固件 ----
    // `default` 用 QEMU 自带的 OpenSBI, 放 DRAM 起点, 运行完跳到 kernel_load_addr。
    args.push("-bios".into());
    args.push(cfg.qemu.bios.clone());

    // ---- 内核 ----
    // 必须用 `-kernel`: `-device loader` 只把 ELF 段写进内存, 不告诉
    // OpenSBI 跳到哪, 固件会跳转到地址 0, QEMU 静默挂住且内核无任何输出。
    // `-kernel` 走 fw_dynamic, 固件由 FW_DYNAMIC 头拿到 next_addr。
    args.push("-kernel".into());
    args.push(elf.display().to_string());

    // ---- 磁盘 ----
    // lab-9 要从磁盘装入用户程序, 块设备必须存在; 无 -drive 时 virtio 探测会失败。
    // 镜像由 `cargo xtask disk` 生成 (内含所有 Rust 用户程序)。
    if run_disk.exists() {
        args.push("-drive".into());
        args.push(format!("file={},if=none,format=raw,id=x0", run_disk.display()));
        args.push("-device".into());
        // virt 机器的 virtio 挂在 virtio-mmio 总线上 (不是 PCI)。
        // bus 槽位 .0 对应 platform 里的 virtio0_base, 两者必须一致。
        args.push("virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0".into());
    }

    // ---- 串口 ----
    // -nographic 把串口 0 接到 stdin/stdout 并关掉图形窗口。
    args.push("-nographic".into());

    // ---- 其他 ----
    // 不要 guest 关机时重启 QEMU。
    args.push("-no-reboot".into());

    if debug {
        // -S: 启动时暂停等 gdb; -gdb tcp::<port>: 指定端口。
        args.push("-S".into());
        args.push("-gdb".into());
        args.push(format!("tcp::{gdb_port}"));
    }

    Ok(args)
}

// 计算 gdb 端口: 用 uid 取模错开同一台机器上不同用户的端口, 避免冲突。
fn gdb_port() -> u16 {
    let uid = unsafe { libc_getuid() };
    (uid % 5000 + 25000) as u16
}

// 读 uid。
//
// # Safety
// `getuid` 是永不失败的系统调用, 不接收指针参数。
unsafe fn libc_getuid() -> u32 {
    // syscall 号 102 = getuid (riscv64 / x86_64 均如此)。用 `extern "C"`
    // 声明比手写 syscall 更可移植 (glibc 处理架构差异)。
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

// 解析 `--timeout <seconds>`。内核不退出, 需要这种"跑 N 秒自动停"
// 的方式供脚本 / CI 使用。
fn opt_timeout(args: &[String]) -> Option<u64> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--timeout" {
            return it.next().and_then(|v| v.parse().ok());
        }
        if let Some(v) = a.strip_prefix("--timeout=") {
            return v.parse().ok();
        }
    }
    None
}

// 带额外选项 (`--timeout`) 的运行入口。
pub fn run_with_options(
    cfg: &Config,
    release: bool,
    verbose: bool,
    extra_args: &[String],
) -> Result<std::process::ExitCode, String> {
    if !cfg.is_qemu() {
        // 开发板无法自动运行, 明确说明并给出部署步骤。
        return Ok(print_board_instructions(cfg, release, verbose)?);
    }

    let elf = build::build_kernel(cfg, release, verbose)?;

    let qemu = qemu_binary(cfg);
    if !which_qemu(&qemu) {
        return Err(format!(
            "找不到 {qemu}。\n\
             请安装对应的 QEMU 系统模拟器 (例如 apt install qemu-system-misc), \
             或者用 QEMU=/path/to/{qemu} 指定路径。"
        ));
    }

    let args = qemu_args(cfg, &elf, false, 0)?;

    println!("\n============================================================");
    println!("  启动 QEMU");
    println!("============================================================");
    println!("  内核     : {}", elf.display());
    println!("  加载地址 : {:#x}", cfg.kernel_load_addr);
    println!("  CPU 数   : {}", cfg.qemu.smp);
    println!("  内存     : {}", cfg.qemu.memory);
    println!("  固件     : -bios {} (QEMU 自带的 OpenSBI)", cfg.qemu.bios);
    println!();
    println!("  【退出 QEMU】先按 Ctrl-A, 再按 X");
    println!("  (Ctrl-C 会被转发给 guest, 不会退出 QEMU)");
    println!("============================================================");
    println!();

    // 打印完整命令行, 学生可复制去手工调试。
    println!("  等效命令:");
    println!("    {} {}", qemu, args.join(" "));
    println!();

    let timeout = opt_timeout(extra_args);
    if let Some(secs) = timeout {
        println!("  【脚本模式】{secs} 秒后自动终止 QEMU (--timeout {secs})");
        println!();
    }

    let mut cmd = Command::new(&qemu);
    cmd.args(&args);
    // QEMU 输出直接接到终端而非捕获: 这是交互式串口会话。
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 {qemu}: {e}"))?;

    // 定时终止: std `Command` 没有超时, 用后台线程 + kill 实现。
    // SIGKILL 不做清理, 正是我们要的 (内核不会退出)。
    if let Some(secs) = timeout {
        let pid = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(secs));
            // SAFETY: pid 是我们 spawn 出来的 QEMU 进程, 归我们所有。
            unsafe {
                libc_kill(pid as i32, 9);
            }
        });
    }

    let status = child
        .wait()
        .map_err(|e| format!("等待 QEMU 退出失败: {e}"))?;

    println!();
    if timeout.is_some() {
        println!("QEMU 已被 --timeout 终止");
        Ok(std::process::ExitCode::SUCCESS)
    } else if status.success() {
        println!("QEMU 已退出 (exit code 0)");
        Ok(std::process::ExitCode::SUCCESS)
    } else {
        println!("QEMU 退出了, 状态: {status}");
        Ok(std::process::ExitCode::from(
            status.code().unwrap_or(1).clamp(0, 255) as u8,
        ))
    }
}

// 给进程发信号。
//
// # Safety
// `pid` 必须是有效进程号, `sig` 是信号编号。
unsafe fn libc_kill(pid: i32, sig: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: 调用者保证 pid 有效。
    unsafe {
        kill(pid, sig);
    }
}

// 以调试模式启动 QEMU (等待 gdb 连接)。
pub fn debug(cfg: &Config, release: bool, verbose: bool) -> Result<std::process::ExitCode, String> {
    if !cfg.is_qemu() {
        return Err(format!(
            "{} 是真实开发板, 无法由 xtask 启动调试。\n\
             开发板调试需要 JTAG 调试器, 本课程不要求。\n\
             QEMU 上的调试请用: cargo xtask debug --config <qemu 配置名>",
            cfg.name
        ));
    }

    let elf = build::build_kernel(cfg, release, verbose)?;
    let port = gdb_port();
    let args = qemu_args(cfg, &elf, true, port)?;

    println!("============================================================");
    println!("  QEMU 已启动, 正在等待 gdb 连接");
    println!("============================================================");
    println!("  内核      : {}", elf.display());
    println!("  gdb 端口  : {port}");
    println!("  符号文件  : {}", elf.display());
    println!();
    println!("  另开一个终端, 执行 (用 gdb-multiarch, 它随发行版自带,");
    println!("  不依赖交叉工具链):");
    println!(
        "    gdb-multiarch -ex 'set architecture {}' {}",
        cfg.facts.gdb_arch, elf.display()
    );
    println!("    (gdb) target remote :{port}");
    println!("    (gdb) info registers");
    println!("    (gdb) x/10i $pc");
    println!();
    if !cfg.facts.gdb.is_empty() {
        println!("  若配置了专用 gdb ({}), 也可以用它:", cfg.facts.gdb);
        println!("    {} {}", cfg.facts.gdb, elf.display());
        println!("    (gdb) target remote :{port}");
        println!();
    }
    println!("  常用断点: 入口 / 主函数 / trap 处理");
    println!("    (gdb) b {}", cfg.facts.entry_symbol);
    println!("    (gdb) b kernel_entry");
    println!("    (gdb) b trap_handler");
    println!("============================================================");
    println!();
    println!("  等效命令:");
    println!("    {} {}", qemu_binary(cfg), args.join(" "));
    println!();

    let qemu = qemu_binary(cfg);
    let mut cmd = Command::new(&qemu);
    cmd.args(&args);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let status = cmd
        .status()
        .map_err(|e| format!("无法启动 {qemu}: {e}"))?;
    Ok(if status.success() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    })
}

// 打印开发板的部署步骤 (不尝试自动烧写)。
// "打印步骤"本身就是成功, 永远返回 SUCCESS, 避免 CI 误判。
fn print_board_instructions(
    cfg: &Config,
    release: bool,
    verbose: bool,
) -> Result<std::process::ExitCode, String> {
    // 先构建并生成镜像, 让学生拿到步骤时镜像已就绪。
    let itb = build::make_image(cfg, release, verbose)?;

    println!();
    println!("============================================================");
    println!("  {} 是真实开发板, xtask 无法替你运行它。", cfg.platform);
    println!("============================================================");
    println!("  已经为你生成好镜像:");
    println!("    {}", itb.display());
    println!();
    println!("  接下来的步骤 (需要你手动完成):");
    for line in cfg.uboot.deploy_hint.lines() {
        if !line.trim().is_empty() {
            println!("  {line}");
        }
    }
    println!();
    println!("  【为什么 xtask 不自动烧卡】");
    println!("    烧写 SD 卡需要 sudo 和一个设备名, 而设备名写错会");
    println!("    清掉你的硬盘。这类危险操作必须由人来确认。");
    println!("============================================================");

    Ok(std::process::ExitCode::SUCCESS)
}

/// 检查某个 QEMU 可执行文件是否可用。
fn which_qemu(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// 捕获 QEMU 输出直到出现目标字符串 (预留给自动化测试 / CI 检查内核输出)。
#[allow(dead_code)]
pub fn capture_until_exit(
    mut cmd: Command,
    timeout: Duration,
    needle: &str,
) -> Result<(bool, String), String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动: {e}"))?;

    let stdout = child.stdout.take().ok_or("无法捕获 stdout")?;
    let reader = BufReader::new(stdout);
    let mut all = String::new();
    let mut found = false;

    for line in reader.lines() {
        let Ok(line) = line else { break };
        all.push_str(&line);
        all.push('\n');
        if line.contains(needle) {
            found = true;
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = timeout;
    Ok((found, all))
}
