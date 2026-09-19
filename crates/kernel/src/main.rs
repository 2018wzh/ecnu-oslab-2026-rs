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
pub mod mm;
pub mod secondary;
pub mod timer;
pub mod trap;
pub mod syscall;
pub mod proc;
pub mod sched;
pub mod sync;
pub mod fs;
// ---- 本阶段模块列表结束 ----

// 由 build.rs 生成: 嵌入的 Rust 用户程序映像。
// 用 include! 而非 `#[path] ... mod`: 生成文件在 cargo 的 OUT_DIR 里
// (不在源码树), 而 #[path] 只接受字面量; include! 接受任意表达式, 可拼
// 出 OUT_DIR 路径。include 进来的 pub static 落在本模块 (名字经下面 use
// 保持可读)。
mod user_images {
    include!(concat!(env!("OUT_DIR"), "/user_images.rs"));
}

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
    if arch::cpu::is_boot_hart() {
        mm::pmem::pmem_init();
    }

    // ---- 分页: 建立并激活内核页表 ----
    // kvm_init 建表, kvm_init_hart 激活, 分开让"建错了"与"装错了"可区分。
    // 必须在物理页分配器之后 (建页表本身要分配物理页)。
    if arch::cpu::is_boot_hart() {
        mm::vm::kvm_init();
    }
    mm::vm::kvm_init_hart();

    // ---- 自检: 把"内存管理对不对"变成看得见的数字 ----
    // 物理页分配器、页表、地址翻译都是看不见的 (做对了没输出)。下面把证据
    // 摆出来与平台常量对照: 空闲页数、页表根页、已映射页数, 以及"恒等映射
    // 翻译结果==输入" —— 这条证明页表真的在工作。
    if arch::cpu::is_boot_hart() {
        let (kfree, ktotal, ufree, utotal) = mm::pmem::pmem_stat();
        oslab_hal::putchar::puts("[oslab-rs] pmem : 内核区 ");
        console::print_dec(kfree);
        oslab_hal::putchar::puts(" / ");
        console::print_dec(ktotal);
        oslab_hal::putchar::puts(" 页空闲, 用户区 ");
        console::print_dec(ufree);
        oslab_hal::putchar::puts(" / ");
        console::print_dec(utotal);
        oslab_hal::putchar::puts(" 页空闲\n");

        if let Some((root, mapped)) = mm::vm::kvm_stat() {
            oslab_hal::putchar::puts("[oslab-rs] kvm  : 根页表 @ ");
            console::print_hex(root);
            oslab_hal::putchar::puts(", 已映射 ");
            console::print_dec(mapped);
            oslab_hal::putchar::puts(" 页\n");
        }

        // 恒等映射: 翻译内核基地址应原样返回。取内核自己的入口地址最合适。
        let probe = arch::cpu::platform().kernel_base;
        match mm::vm::kvm_translate(probe) {
            Some(pa) if pa == probe => {
                oslab_hal::putchar::puts("[oslab-rs] kvm  : 地址翻译自检 ");
                console::print_hex(probe);
                oslab_hal::putchar::puts(" -> ");
                console::print_hex(pa);
                oslab_hal::putchar::puts(" (恒等映射, 正确)\n");
            }
            Some(pa) => {
                oslab_hal::putchar::puts("[oslab-rs] kvm  : 地址翻译自检失败 ");
                console::print_hex(probe);
                oslab_hal::putchar::puts(" -> ");
                console::print_hex(pa);
                oslab_hal::putchar::puts(" (期望恒等映射)\n");
            }
            None => {
                oslab_hal::putchar::puts("[oslab-rs] kvm  : 地址翻译自检失败 (没有映射)\n");
            }
        }
    }

    // ---- 进程与调度 ----
    // 放在分页之后: 进程需要内核栈 (来自物理页分配器) 与页表; 顺序反了
    // 症状是"进程一创建就缺页"。属于 lab-4 (进程表与调度器是创建进程前提)。
    if arch::cpu::is_boot_hart() {
        proc::proc_init();
    }
    proc::sched_init_hart();

    // ---- 建立真正的 trap 处理 ----
    // 必须在打开中断之前: 顺序反了, 已使能的中断可能在 stvec 还是停车点时
    // 到来 → 启动中途卡在死循环里, 无任何输出。
    trap::init();

    // ---- 定时器: 每个 hart 装自己的第一次闹钟 ----
    // 定时器是 per-hart 资源 (sie 与 mtimecmp 都是 per-hart), 只让启动核装
    // 闹钟→从核收不到时钟中断 (无调度器时看不出, 引入调度器后"某核进程再也
    // 换不出去")。必须排在 trap::init 之后 (闹钟装上随时可能触发)。
    timer::timer_create();

    // 使能"时钟中断这一类" (四级使能第 3 级); 总开关在下面的阶段 8 才打开。
    arch::irq::enable_timer();

    // ---- 启动其他 hart ----
    // 用 SBI HSM。必须检查返回值: 在不存在的 hart 上调用会返回错误, 忽略则
    // "某个核永远起不来"且无日志。
    if arch::cpu::is_boot_hart() {
        // 传入从核入口地址。hal 不能依赖 kernel (方向相反), 所以此地提供。
        let started = arch::smp::start_others(arch::SECONDARY_ENTRY as *const () as usize);
        oslab_hal::putchar::puts("[oslab-rs] smp: requested ");
        console::print_dec(arch::cpu::platform().ncpu - 1);
        oslab_hal::putchar::puts(" secondary hart(s), ");
        console::print_dec(started);
        oslab_hal::putchar::puts(" accepted by firmware, ");
        console::print_dec(arch::smp::count_online_harts());
        oslab_hal::putchar::puts(" now online\n");
    }

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
        block_selfcheck();
    }

    // ---- 阶段 10: 创建第一个用户进程并进入用户态 ----
    // 只启动核做 (创建进程/分配用户页/建 trapframe 都是全局动作, 每个 hart
    // 做一遍会得到 N 个 init 互相覆盖进程表)。其他 hart 直接进 idle (真实
    // SMP 调度器会让空闲核去"偷"别的核的进程, 见 lab-6)。
    if arch::cpu::is_boot_hart() {
        launch_first_user_program();
    }

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
fn block_selfcheck() {
    // 不再需要 BlockDevice: 设备由 drivers 层选好并返回一个 trait 对象。
    use oslab_drivers::block::BlockError;
    use oslab_hal::putchar::puts;

    let plat = arch::cpu::platform();
    puts("[oslab-rs] 块设备自检: 按平台描述初始化\n");

    // "哪种设备"→"用哪个驱动"的映射在 drivers/ 里, kernel 只拿到 trait
    // 对象, 加一种块设备 kernel 不改一行 (所以不会出现具体驱动的名字)。
    //
    // SAFETY: 启动阶段只调用一次; 设备地址在 kvm_init 时已映射。
    let dev = match unsafe { oslab_drivers::block::init_default(plat) } {
        Ok(d) => d,
        Err(e) => {
            puts("[oslab-rs]   块设备初始化失败: ");
            puts(match e {
                BlockError::NotReady => "设备未就绪 (没插卡 / 没挂磁盘?)",
                BlockError::Timeout => "超时",
                BlockError::DeviceError => "设备报告错误",
                BlockError::OutOfRange => "块号越界",
                BlockError::BadBufferSize => "缓冲区大小不对",
                BlockError::Unsupported => "未实现",
            });
            puts("\n");
            return;
        }
    };

    puts("[oslab-rs]   设备: ");
    puts(dev.name());
    puts("  容量: ");
    console::print_dec(dev.capacity_sectors() as usize);
    puts(" 扇区\n");

    // 读第 0 块 (超级块)。先打印"开始读": read 可能卡在等待设备完成; 没有
    // 这行,"读失败"与"读卡住"在串口上看一模一样 (都没下文)。
    puts("[oslab-rs]   开始读块 0 ...\n");

    let mut buf = [0u8; 512];
    match dev.read(0, &mut buf) {
        Ok(()) => {
            let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            puts("[oslab-rs]   读块 0 成功, 超级块魔数 = ");
            console::print_hex(magic as usize);
            if magic == 0x1020_3040 {
                puts("  <- 与 mkfs 写入的一致, 块设备通路正常\n");
            } else {
                puts("  <- 与期望的 0x10203040 不符!\n");
            }
            // ---- lab-8 自检: 挂载文件系统并列出根目录 ----
            // inode 读取与目录解析属于"做对了没输出、做错了只是后面某步莫名
            // 奇炒地坏掉"的工作, 所以单独验证这一层。
            fs_selfcheck(dev);

            // ---- 从磁盘装入用户程序并运行 (lab-9) ----
            // 走通后下面"嵌入映像"的装载不会执行到 (enter_user 不返回)。
            // 保留它作为 lab-8 及更早阶段的路径。
            let _ = launch_from_disk(dev, "init");
        }
        Err(e) => {
            puts("[oslab-rs]   读块 0 失败: ");
            // 打印具体错误变体, 区分"队列未建立/设备不响应/参数越界"。
            puts(match e {
                BlockError::NotReady => "NotReady (队列未建立)",
                BlockError::Timeout => "Timeout (设备未在预期时间内完成)",
                BlockError::DeviceError => "DeviceError (设备报告了错误状态)",
                BlockError::OutOfRange => "OutOfRange (块号越界)",
                BlockError::BadBufferSize => "BadBufferSize (缓冲区大小不对)",
                BlockError::Unsupported => "Unsupported (未实现)",
            });
            puts("\n");
        }
    }
}

/// 挂载文件系统, 从磁盘读出一个 ELF 用户程序并运行它 —— lab-9 核心。
///
/// 整条链路是: 块设备 → 缓冲缓存 → fs(inode/目录) → 按路径读 ELF →
/// 加载器按 program header 装 → 进 U-mode。用 ELF 而非扁平二进制, 因为
/// 扁平要求"文件偏移==虚拟地址偏移"(一条约定, 链接脚本稍有不慎就
/// 破坏); ELF 把入口与每段位置显式写进文件, 不存在"约定被破坏"这种失败。
fn launch_from_disk(dev: &'static mut dyn oslab_drivers::block::BlockDevice, prog: &str) -> bool {
    use oslab_hal::putchar::puts;

    puts("[oslab-rs] 从磁盘装载用户程序 (");
    puts(prog);
    puts(")...\n");

    // ---- 1. 挂载文件系统 ----
    let mut fs = match crate::fs::mount::Fs::mount(dev) {
        Ok(fs) => fs,
        Err(e) => {
            puts("[oslab-rs]   挂载失败: ");
            puts(match e {
                crate::fs::mount::MountError::Io => "读超级块失败",
                crate::fs::mount::MountError::BadMagic => "魔数不对 (忘 mkfs?)",
                crate::fs::mount::MountError::BadSuperblock => "超级块不自洽",
                crate::fs::mount::MountError::BadRoot => "根目录不是目录",
            });
            puts("\n");
            return false;
        }
    };

    // ---- 2. 按路径找到它 ----
    let mut path = [0u8; 32];
    path[0] = b'/';
    let n = prog.len().min(30);
    path[1..1 + n].copy_from_slice(&prog.as_bytes()[..n]);
    let path = &path[..1 + n];

    let Some(inum) = fs.lookup(path) else {
        puts("[oslab-rs]   磁盘上找不到 ");
        puts(prog);
        puts("\n");
        return false;
    };
    let inode = fs.read_inode(inum);
    puts("[oslab-rs]   找到 ");
    puts(prog);
    puts(": inode ");
    console::print_dec(inum as usize);
    puts(", ");
    console::print_dec(inode.size as usize);
    puts(" 字节\n");

    // ---- 3. 把整个文件读进内存 ----
    // 用静态缓冲: 用户程序最大 70KB, 放栈上会写穿 4KiB 内核栈。静态区零
    // 初始化且不随调用栈变化。
    const IMG_MAX: usize = 72 * 1024;
    static mut IMG: [u8; IMG_MAX] = [0u8; IMG_MAX];
    // SAFETY: 启动阶段只有一个 hart 在用这块缓冲。
    let img = unsafe { &mut *core::ptr::addr_of_mut!(IMG) };

    let (read, complete) = fs.read_file(&inode, img);
    if !complete {
        puts("[oslab-rs]   读文件不完整 (\n");
        return false;
    }
    puts("[oslab-rs]   已读入 ");
    console::print_dec(read);
    puts(" 字节\n");

    // ---- 3.5 把文件系统安装成全局的 ----
    // 系统调用 (open/read/exec) 之后都通过它访问磁盘; 必须在创建进程之前
    // (进程一创建就要建 fd 0/1/2)。放这里是因为上面的 lookup/read_file 还
    // 需本地 `fs` 的可变借用; 装到全局后只能经 mount::fs() 访问。
    crate::fs::mount::install(fs);

    // ---- 4. 创建一个进程, 用 ELF 加载器装入 ----
    let Some(p) = proc::proc_alloc() else {
        puts("[oslab-rs]   进程表已满\n");
        return false;
    };
    let pid = p.pid;
    // SAFETY: pid 刚由 proc_alloc 返回。
    unsafe {
        proc::set_current(pid);
    }

    match proc::user::proc_make_user_elf(pid, &img[..read]) {
        Ok(entry) => {
            puts("[oslab-rs]   ELF 入口 = ");
            console::print_hex(entry as usize);
            puts(", 进程 pid=");
            console::print_dec(pid);
            puts(" 已就绪, 切换到用户态...\n\n");
        }
        Err(e) => {
            puts("[oslab-rs]   ELF 装载失败: ");
            puts(match e {
                proc::elf::ElfError::TooSmall => "文件太小",
                proc::elf::ElfError::BadMagic => "不是 ELF (魔数不对)",
                proc::elf::ElfError::Not64 => "不是 64 位 ELF",
                proc::elf::ElfError::NotLittleEndian => "不是小端序",
                proc::elf::ElfError::WrongMachine => "不是 RISC-V 目标文件",
                proc::elf::ElfError::OutOfUserRange => "段落在用户地址空间之外",
                proc::elf::ElfError::LoadFailed => "分配物理页失败",
                proc::elf::ElfError::NoLoadableSegment => "没有可装载的段",
            });
            puts("\n");
            return false;
        }
    }

    // ---- 5. 进入用户态 (不返回) ----
    // SAFETY: proc_make_user_elf 已准备好 trapframe 与页表映射。
    unsafe {
        proc::user::enter_user();
    }
}

/// 挂载文件系统并列出根目录 —— lab-8 验收点。
///
/// inode 读取与目录解析是"做对了没输出"的工作, 所以单独验证: 挂载 →
/// 读根目录 inode → 用缓冲缓存读内容 (含间接块) → 逐个解释 dirent。一行
/// `inum=N name` 同时证明后两步都对。
fn fs_selfcheck(dev: &mut dyn oslab_drivers::block::BlockDevice) {
    use oslab_hal::putchar::puts;

    let mut fs = match crate::fs::mount::Fs::mount(dev) {
        Ok(fs) => fs,
        Err(e) => {
            puts("[oslab-rs] fs   : 挂载失败 (");
            puts(match e {
                crate::fs::mount::MountError::Io => "读超级块失败",
                crate::fs::mount::MountError::BadMagic => "魔数不对 (忘 mkfs?)",
                crate::fs::mount::MountError::BadSuperblock => "超级块不自洽",
                crate::fs::mount::MountError::BadRoot => "根目录不是目录",
            });
            puts(")\n");
            return;
        }
    };

    puts("[oslab-rs] fs   : 超级块 ");
    console::print_dec(fs.sb.size as usize);
    puts(" 块, ");
    console::print_dec(fs.sb.ninodes as usize);
    puts(" 个 inode\n");

    // 目录内容是定长 dirent (2 字节 inode 号小端 + 14 字节名字, 不足补 0),
    // 与 docs/abi-spec.md 和 xtask 的 mkfs 一致 —— 定长记录没有边界可算错。
    let root = fs.read_inode(crate::fs::mount::ROOTINO);
    let mut buf = [0u8; 4096];
    let (n, complete) = fs.read_file(&root, &mut buf);
    if !complete {
        puts("[oslab-rs] fs   : 读根目录失败\n");
        return;
    }

    puts("[oslab-rs] fs   : 根目录内容:\n");
    let mut off = 0;
    while off + 16 <= n {
        let inum = u16::from_le_bytes([buf[off], buf[off + 1]]) as u32;
        let name = &buf[off + 2..off + 16];
        // inum==0 表示槽位为空 (文件被删过), 直接跳过。
        if inum != 0 {
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            puts("                inum=");
            console::print_dec(inum as usize);
            puts("  ");
            for &c in &name[..end] {
                oslab_hal::putchar::putc(c);
            }
            puts("\n");
        }
        off += 16;
    }
}

/// 创建第一个用户进程, 然后把 CPU 交给用户态。
///
/// 链路: proc_alloc (槽+内核栈+trapframe 位置) → set_current → 
/// proc_make_user (把 Rust 用户程序映像装入用户页, trapframe 填成"像刚从
/// 用户态陷入") → enter_user (切页表 + sret 进 U-mode, 不返回)。
///
/// 把用户映像"嵌入"而非从磁盘读: 本阶段目标是证明用户态通路通; 若同时依赖
/// 块设备/fs/ELF 三件事, 任何错的现象都一样 ("什么都没有输出") 无法定位。
fn launch_first_user_program() {
    use oslab_hal::putchar::puts;

    // ---- 检查映像是否可用 ----
    // 空映像会让 proc_make_user 分配 0 代码页、把用户 PC 设成怪地址、进用户态
    // 立刻缺页 ("执行了一条指令就崩"), 与真正原因 (忘建用户程序) 相距很远,
    // 所以显式检查并打印指引。
    if !user_images::USER_IMAGE_INIT_FOUND {
        puts("[oslab-rs] 没有找到用户程序映像 target/user/init.bin\n");
        puts("[oslab-rs] 请先运行: cargo xtask disk --config <name>\n");
        puts("[oslab-rs] 然后重新构建内核。现在进入 idle。\n");
        return;
    }

    puts("[oslab-rs] 正在装载用户程序 (Rust, U-mode)...\n");
    puts("[oslab-rs]   映像大小: ");
    console::print_dec(user_images::USER_IMAGE_INIT.len());
    puts(" 字节\n");

    // ---- 1. 分配进程槽 ----
    let Some(p) = proc::proc_alloc() else {
        puts("[oslab-rs] FATAL: 进程表已满, 无法创建 init\n");
        return;
    };
    let pid = p.pid;

    // ---- 2. 设为当前进程 ----
    // 必须在 proc_make_user 之前: 它 (及后面的 enter_user) 都用 current()。
    //
    // SAFETY: pid 刚由 proc_alloc 返回, 是合法进程号。
    unsafe {
        proc::set_current(pid);
    }

    // ---- 3. 装入映像并准备 trapframe ----
    if !crate::mm::uvm::proc_make_user(pid, user_images::USER_IMAGE_INIT) {
        puts("[oslab-rs] FATAL: 装入用户程序失败 (内存不足?)\n");
        return;
    }

    puts("[oslab-rs] init (pid=");
    console::print_dec(pid);
    puts(") 已就绪, 切换到用户态...\n\n");

    // ---- 4. 进入用户态 ----
    // 不返回: 切页表并 sret, CPU 从此在 U-mode 运行; 用户程序系统调用时会
    // 重新陷入内核 (走 trap_entry)。
    //
    // SAFETY: proc_make_user 已准备好 trapframe; 调用者是启动核, 处于可切换
    // 特权级的上下文。
    unsafe {
        proc::user::enter_user();
    }
}

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
    arch::irq::enable();
    let is_boot_hart = arch::cpu::is_boot_hart();
    let mut last_ticks = timer::timer_ticks();

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
        if is_boot_hart {
            let now = timer::timer_ticks();
            if now != last_ticks {
                timer::timer_print_ticks();
                last_ticks = now;
            }
        }

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