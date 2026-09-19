//! ECNU OSLab 2026 (Rust) 内核 —— OS 语义所在地 (调度、内存策略、文件
//! 系统、系统调用)。这里不出现硬编码 MMIO 地址 (来自 platform)、CSR
//! 名 (属于 arch, 只用 `arch::irq::disable()` 这类语义化接口)、平台路径
//! (平台由 cargo feature 选择) 或具体设备寄存器操作 (在 drivers)。
//!
//! 判断改动是否越界: "换到 VisionFive2 (不同 DRAM 基址/hart 区间/块设备)
//! 这段代码还能原样工作吗?" 能 -> 放这里; 不能 -> 是平台/架构/驱动的事实。
//! 这一判据可检验: `cargo xtask build --config riscv64-visionfive2` 会用另一
//! 套常量重编译本 crate, 写死的假设立刻暴露。
#![no_std]
#![no_main]
// `unsafe_op_in_unsafe_fn`: 在 unsafe fn 内部也要显式写 unsafe {} —— 让
// "这个 unsafe 块到底在 unsafe 什么"变得可见, 在人人写 unsafe 的裸机仓库
// 里这条纪律很值。
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

// ---------------------------------------------------------------------------
// 编译期生成的文件
// ---------------------------------------------------------------------------
// `CONFIG_*` 常量来自 build.rs 对 configs/*.toml 的解析。用 include! 而不是
// 放进 src/: 它是生成物, 不该被 git 跟踪, 也不该被 rust-analyzer 当作源。
pub mod config {
    //! 构建期从 `configs/*.toml` 生成的平台常量 —— "构建系统对这台机器的
    //! 认识"。运行期的 `oslab_hal::platform::PLATFORM` 是"代码对这台机器的
    //! 认识", 两者由各自的构建期检查保证一致。
    include!(concat!(env!("OUT_DIR"), "/platform_facts.rs"));
}

// ---- 各子系统模块 ----
pub mod console;
pub mod panic;
// ---- 本阶段模块列表结束 ----

// 由 build.rs 生成: 嵌入的 Rust 用户程序映像。
// 用 include! 而非 `#[path] ... mod`: 生成文件在 cargo 的 OUT_DIR 里
// (不在源码树), 而 #[path] 只接受字面量; include! 接受任意表达式, 可拼
// 出 OUT_DIR 路径。include 进来的 pub static 落在本模块 (名字经下面 use
// 保持可读)。

// ---------------------------------------------------------------------------
// 内核入口 (由 hal 的启动汇编调用)
// ---------------------------------------------------------------------------

use oslab_hal::arch;

/// 内核的 Rust 入口。
///
/// 由 `_entry` 汇编完成三件事后调用: 校验 hartid、建立本 hart 内核栈、
/// hartid 存进 tp。此时 satp=0 (分页关)、sstatus.SIE=0 (中断关)、tp=hartid。
/// 签名 `extern "C" fn() -> !` 无参数: 汇编写 `call kernel_entry`, hartid
/// 经 tp 传递 (a0 是 caller-saved, 汇编与 Rust 间无类型信息帮编译器保存)。
#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry() -> ! {
    // ---- 阶段 1: 最早的输出走 SBI ----
    // 不用 UART 驱动: 它需要 platform 常量地址正确, 而"地址正确"正是要
    // 验证的内容。"内核跑起来了"与"板级常量对"于是分成两条可区分的证据。
    let hartid = arch::cpu::hartid();
    let cpuid = arch::cpu::cpu_id();

    oslab_hal::putchar::puts("\n");
    oslab_hal::putchar::puts("[oslab-rs] kernel entry reached (SBI console)\n");

    // ---- 阶段 2: 校验 hartid ----
    // 汇编已查过一次, 这里用 platform 层同源再查: 将来若有人删掉汇编里
    // 的检查, 这里仍拦住在非法 hart 上运行, 而不是让内核写别人的内存。
    let Some(cpuid) = cpuid else {
        oslab_hal::putchar::puts(
            "[oslab-rs] FATAL: this hart is not in the platform's hart range\n",
        );
        oslab_hal::putchar::puts("[oslab-rs] hart id = ");
        console::print_dec(hartid);
        oslab_hal::putchar::puts(" is reserved or out of range; parking.\n");
        arch::time::park_current_hart();
    };

    // ---- 阶段 2.5: 我不是冷启动核 -> 本 hart 停车 ----
    // 冷启动核是运行期事实 (由启动汇编一次原子抽签定), 不是平台常量。
    // 走到这里的 hart 是固件把多个 hart 都送进内核的; 停车即可, 之后会
    // 由冷启动核用 SBI HSM 正式启动 (走 secondary_entry)。
    // boot_claim_ok 同时区分"启动核正好是 hart 0"(记录值 1) 与"抽签根本
    // 没发生"(= .bss 没被清, 记录值 0, 未定义状态必须停)。
    if !arch::cpu::boot_claim_ok() {
        oslab_hal::putchar::puts(
            "[oslab-rs] FATAL: 启动抽签没有发生 (.bss 未被清零) —— 检查 boot.rs 的抽签逻辑\n",
        );
        oslab_hal::arch::time::park_current_hart();
    }

    if !arch::cpu::is_boot_hart() {
        oslab_hal::putchar::puts("[oslab-rs] hart ");
        console::print_dec(hartid);
        oslab_hal::putchar::puts(" 不是冷启动核 (冷启动核是 hart ");
        console::print_dec(arch::cpu::cold_boot_hart());
        oslab_hal::putchar::puts("), 本 hart 停车\n");
        oslab_hal::putchar::puts(
            "[oslab-rs]   (它会由冷启动核用 SBI HSM 正式启动, 这一行可以忽略)\n",
        );
        arch::time::park_current_hart();
    }

    // ---- 阶段 3: 安装 trap 向量之前的停车点 ----
    // stvec 还 0 时若发生 trap, CPU 跳地址 0 (QEMU 上静默死机)。装上停车点
    // 后变成"明确卡在这个函数里", gdb 看 pc 就知道发生了什么。把危险路径
    // 变成安全的、可观察的死循环是启动代码里常见的做法。
    //
    // SAFETY: park_forever 不返回、不用任何寄存器, 满足 trap 入口最低要求。
    unsafe {
        arch::trap::install_vector(arch::trap::park_forever);
    }

    // ---- 阶段 4: 只有启动核做全局初始化 ----
    // 判断依据是 hartid == platform.boot_hart 而非 cpuid == 0: 两者在 QEMU
    // 等价 (boot_hart=0), 但在 VF2 上不同 (boot_hart=1, cpuid=0)。用 hartid
    // 表达"固件把控制权交给了谁"才是真语义。
    if arch::cpu::is_boot_hart() {
        boot_banner(cpuid);
    }

    // ---- 阶段 5: 初始化串口, 之后所有输出走真正的硬件 ----
    let uart = oslab_drivers::serial::init_console();

    if arch::cpu::is_boot_hart() {
        // 自检结果信息量最大的一行: 证明 platform 的 UART 地址对 (驱动能读到
        // 刚写进去的 LCR)、波特率分频对 (你能读懂这句话)、SBI→真实串口的路径通。
        oslab_hal::putchar::puts("[oslab-rs] uart16550 ready @ ");
        console::print_hex(uart.base());
        oslab_hal::putchar::puts(" divisor=");
        console::print_dec(uart.divisor() as usize);
        oslab_hal::putchar::puts(" irq=");
        console::print_dec(uart.irq() as usize);
        oslab_hal::putchar::puts("\n");
    }

    // ---- 物理内存: 建立页分配器 ----
    // 必须在分页之前 (分页要分配页表页); 只启动核做 (会重建整个空闲链表,
    // 两核同跑会把链表串成两半)。





    // ---- 阶段 8: 配置本 hart 的中断 ----
    // 放最后: 打开总开关后任何使能的中断都可能立刻来, 而处理函数必须已就绪。
    arch::irq::enable_source(arch::trap::source::SOFTWARE); // 核间中断
    let _ = uart;

    if arch::cpu::is_boot_hart() {
        oslab_hal::putchar::puts("\n");
        oslab_hal::putchar::puts("[oslab-rs] boot complete.\n");
    }

    // ---- 阶段 9.5: 块设备自检 ----
    // 探测 virtio 槽位、读超级块校验魔数: 把"驱动是否正确"变成立刻可见的
    // 结果, 而非等到 fs 挂载时才间接暴露。


    if arch::cpu::is_boot_hart() {
        oslab_hal::putchar::puts("[oslab-rs] 进入 idle 循环\n");
    }

    // ---- 阶段 9: 进入 idle ----
    // 当前阶段无调度器, 所以是带周期性时钟中断的等待循环, 同时验证定时器路径通。
    idle_loop(cpuid)
}

/// 打印启动横幅。
///
/// 每一项都是在"出示证据" (自检报告): 平台名、DRAM 区间、内核加载地址
/// (与 _entry 实际地址对比)、固件基址、CPU/hart 区间、UART 地址中断时钟。
/// 学生把这份输出与 docs/ports/*.md 的地址清单对照, 确认内核与板子对得上
/// —— 这是"不用设备树"要求的补偿: 常量必须能人工核对, 前提是打印出来。
fn boot_banner(cpuid: usize) {
    let plat = arch::cpu::platform();

    oslab_hal::putchar::puts("\n");
    oslab_hal::putchar::puts("========================================================\n");
    oslab_hal::putchar::puts("  ECNU OSLab 2026  (Rust)\n");
    oslab_hal::putchar::puts("========================================================\n");
    // 按"身份 / 内存 / CPU / 设备"各占一行, 一行放齐该系统要核对的全部数字。
    oslab_hal::putchar::puts("  ");
    oslab_hal::putchar::puts(crate::config::CONFIG_NAME);
    oslab_hal::putchar::puts("  |  ");
    oslab_hal::putchar::puts(plat.name);
    oslab_hal::putchar::puts("  arch/boot ");
    oslab_hal::putchar::puts(plat.arch);
    oslab_hal::putchar::puts("/");
    oslab_hal::putchar::puts(crate::config::CONFIG_BOOT);
    oslab_hal::putchar::puts("\n");

    oslab_hal::putchar::puts("  DRAM ");
    console::print_hex(plat.dram_base);
    oslab_hal::putchar::puts("+");
    console::print_size_mib(plat.dram_size);
    oslab_hal::putchar::puts("  kernel ");
    console::print_hex(plat.kernel_base);
    oslab_hal::putchar::puts(" (config ");
    console::print_hex(crate::config::CONFIG_KERNEL_LOAD_ADDR);
    oslab_hal::putchar::puts(")  firmware ");
    console::print_hex(plat.firmware_base);
    oslab_hal::putchar::puts("..");
    console::print_hex(plat.kernel_base);
    oslab_hal::putchar::puts("\n");

    oslab_hal::putchar::puts("  cpus ");
    console::print_dec(plat.ncpu);
    oslab_hal::putchar::puts(" hart [");
    console::print_dec(plat.harts.min);
    oslab_hal::putchar::puts("..");
    console::print_dec(plat.harts.max);
    // "启动核"是固件实际交给内核的 hart (运行期, 抽签记录); "平台配置"是
    // 平台层声明的预期。两者不同说明固件没按约定启动 (仍能正常启动), 值得看见。
    oslab_hal::putchar::puts("]  启动核 ");
    console::print_dec(arch::cpu::cold_boot_hart());
    oslab_hal::putchar::puts(" (平台配置 ");
    console::print_dec(plat.boot_hart);
    oslab_hal::putchar::puts(")  this hart hartid ");
    console::print_dec(arch::cpu::hartid());
    oslab_hal::putchar::puts(" cpuid ");
    console::print_dec(cpuid);
    if arch::cpu::is_boot_hart() {
        oslab_hal::putchar::puts(" (冷启动核)");
    } else {
        oslab_hal::putchar::puts(" (从核)");
    }
    let (maj, min) = arch::sbi::spec_version();
    oslab_hal::putchar::puts("  sbi ");
    console::print_dec(maj);
    oslab_hal::putchar::puts(".");
    console::print_dec(min);
    oslab_hal::putchar::puts("\n");

    oslab_hal::putchar::puts("  uart0 ");
    console::print_hex(plat.uart0_base);
    oslab_hal::putchar::puts(" irq ");
    console::print_dec(plat.uart0_irq as usize);
    oslab_hal::putchar::puts(" clock ");
    console::print_dec(plat.uart0_clock as usize);
    oslab_hal::putchar::puts("Hz  plic ");
    console::print_hex(plat.plic_base);
    match plat.block {
        oslab_hal::platform::BlockKind::VirtioMmio => {
            oslab_hal::putchar::puts("  block virtio-mmio@");
            console::print_hex(plat.virtio0_base);
            oslab_hal::putchar::puts(" irq ");
            console::print_dec(plat.virtio0_irq as usize);
            oslab_hal::putchar::puts(" slots ");
            console::print_dec(plat.virtio_count);
        }
        oslab_hal::platform::BlockKind::DesignWareMshc => {
            oslab_hal::putchar::puts("  block dw-mshc(sd)@");
            console::print_hex(plat.sdhci_base);
        }
    }
    // 两平台时间源同一套: `time` CSR 读, SBI 设置下一次中断。
    oslab_hal::putchar::puts("  timer ");
    console::print_dec(plat.timer_interval);
    oslab_hal::putchar::puts(" ticks (rdtime + SBI)\n");
}

/// 块设备自检: 探测 virtio、建立队列、读第 0 块并校验超级块魔数。
///
/// 专门写自检, 因为"块设备能读"是 fs/exec/从磁盘装用户程序整条链的地基;
/// 它坏了上层全失败且现象都是"什么都没有输出", 无从分辨是哪层。一个
/// "读一块并检查魔数"的自检把这层单独验证了。

/// 创建第一个用户进程, 然后把 CPU 交给用户态。
///
/// 链路: proc_alloc (槽+内核栈+trapframe 位置) → set_current → 
/// proc_make_user (把 Rust 用户程序映像装入用户页, trapframe 填成"像刚从
/// 用户态陷入") → enter_user (切页表 + sret 进 U-mode, 不返回)。
///
/// 把用户映像"嵌入"而非从磁盘读: 本阶段目标是证明用户态通路通; 若同时依赖
/// 块设备/fs/ELF 三件事, 任何错的现象都一样 ("什么都没有输出") 无法定位。
/// lab-7/8/9 换成分块读 ELF (那时有文件系统)。

/// 每个 hart 的等待循环。
///
/// 用 `wfi` 而非空转: 让 CPU 进入低功耗直至下一个中断 (真机上省电且便于
/// JTAG 接管; 在 QEMU 上近乎等价 nop)。每 100000 轮检查一次栈 canary:
/// 单次检查成本是一次内存读, 换来栈溢出在下一轮被报告, 而非几百万条指令
/// 后以无关症状出现 —— 用一点运行期成本换可诊断的失败模式, 内核开发里
/// 几乎总划算 (调试内核的时间远贵于 CPU 时间)。
fn idle_loop(cpuid: usize) -> ! {
    let mut ticks: usize = 0;

    // ---- 让时钟中断真的进来, 并把它变成可见的证据 ----
    // 必须显式开总开关: 到这一步只使能了 sie.STIE (第 3 级), sstatus.SIE 总
    // 开关还没开过 (lab-3 没别处会开; lab-4 起由"从用户态返回"顺手开)。不开
    // 总开关, wfi 立即返回、时钟中断一个都不来 → 屏幕只剩 "boot complete"。
    // 用轮询而非在中断里打印: 中断上下文里做串口要抢锁还可能被嵌套; 把记账
    // 与显示分开后, 写坏的定时器最多让计数不对, 不可能把串口刷爆。

    loop {
        // 等待中断。
        //
        // SAFETY: wfi 在中断关闭时立即返回 (然后循环), 打开时让 CPU 睡到下一
        // 个中断; 两种情况都不破坏状态。
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }

        ticks = ticks.wrapping_add(1);

        // 计数变了 -> 刚刚真的收到了一次时钟中断。只在启动核打印, 否则多个
        // hart 的计数交错成无法解读的一串 (看起来像计数器坏, 实为两序列混着)。

        // 每 100000 轮查一次栈 canary (不全查: wfi 无中断时立即返回, 循环可
        // 转得飞快, 每轮查浪费可观 CPU)。
        if ticks % 100_000 == 0 && !arch::boot::check_stack_canary(cpuid) {
            oslab_hal::putchar::puts("\n[oslab-rs] FATAL: kernel stack overflow on cpu ");
            console::print_dec(cpuid);
            oslab_hal::putchar::puts("\n");
            crash_report();
        }
    }
}

/// 打印一份崩溃现场的最小信息, 然后停车。
///
/// 刻意只打印不依赖复杂状态的东西 —— 调用时内核已不健康, 任何依赖锁/分配/
/// 页表的操作都可能让情况更糟。这里不出现任何 CSR 名 (kernel 只知道"上次
/// 陷阱的原因/位置/地址", 不知它们存在哪个寄存器里)。
pub fn crash_report() -> ! {
    // 一次取全 trap 现场 (理由见 hal::arch::trap::last_fault_info)。
    let fault = arch::trap::last_fault_info();

    oslab_hal::putchar::puts("--------------------------------------------------------\n");
    oslab_hal::putchar::puts("  hartid=");
    console::print_dec(arch::cpu::hartid());
    oslab_hal::putchar::puts("  cause=");
    oslab_hal::putchar::puts(fault.cause_name);
    oslab_hal::putchar::puts("  raw=");
    console::print_hex(fault.cause_raw);
    oslab_hal::putchar::puts("  pc=");
    console::print_hex(fault.pc);
    oslab_hal::putchar::puts("  addr=");
    console::print_hex(fault.address);
    oslab_hal::putchar::puts("\n");
    oslab_hal::putchar::puts("--------------------------------------------------------\n");

    arch::time::park_current_hart()
}