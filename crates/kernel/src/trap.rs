//! trap 处理 (OS 语义侧)。分工:
//!   hal::arch::trap_entry  架构侧汇编入口 (CSR 名、原子换栈、sret)
//!   hal::arch::trap        架构侧: trapframe 布局、原因解码、返回时 sstatus
//!   kernel::trap (本文件)  OS 侧: 收到某类 trap 之后**做什么**
//! 例如"缺页要不要按需分配"是 OS 语义在这里; "缺页异常编号是 13"是
//! 架构事实在 arch。陷阱入口不放 kernel: 它是清一色 CSR 名与 RISC-V
//! 指令, 留在 kernel 会让"kernel 不允许出现 CSR 名"这条规则出现第一个
//! 例外 (第一个例外之后就是无数个)。与汇编的接口只有一个 `trap_handler`。
//!
//! trapframe 偏移由 `offset_of!` 提供 (唯一来源是结构体定义本身), 避免
//! C 版本"手写偏移 + 总大小断言守不住字段顺序改坏"的问题。

use oslab_hal::arch::{self as hal_arch, trap as arch_trap, TrapCause, TrapFrame};

use crate::console;
use crate::timer;

// ===========================================================================
// 初始化
// ===========================================================================

/// 安装真正的 trap 向量。
///
/// 在 [`crate::kernel_entry`] 里设备初始化之后、开中断之前调用。
/// 每个 hart 都要调用: `stvec` 是 per-hart CSR, 从核带 stvec=0 收中断
/// 会跳地址 0。
pub fn init() { }

unsafe extern "C" {
    /// 从一个 trapframe 返回到它描述的特权级。
    ///
    /// 不返回: 执行 `sret` 跳去 trapframe 里的 sepc。进程切换时会用它
    /// 对新进程的 trapframe 调用, 从而"返回"到一个从未运行过的上下文。
    pub fn trap_return(tf: *mut TrapFrame);
}

// ===========================================================================
// Rust 处理函数
// ===========================================================================

/// trap 的 Rust 处理函数, 由汇编调用, 参数是 trapframe 指针。
///
/// 必须做到: 不改 sp (汇编返回后要从 sp 重新取 trapframe 指针); 返回前
/// 把 `tf.status` 改成目标状态 (汇编用 `csrw sstatus, tf.status` 设 sret
/// 行为, 忘了改则 SPP 可能是 1, 返回跳回 S-mode 静默卡死)。
/// 参数用裸指针: 汇编传的就是地址, 不会被 Rust 引用同时借用。
#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(tf_ptr: *mut TrapFrame) {
    // SAFETY: 由汇编保证 tf_ptr 指向栈上已初始化的 TrapFrame, 且本函数
    // 执行期间不会有第二个引用 (每 hart 独立内核栈, trap 处理串行)。
    let tf = unsafe { &mut *tf_ptr };

    let cause = arch_trap::last_fault_cause();

    match cause {
        // -----------------------------------------------------------------
        // 时钟中断: 重新设置下一次中断, 然后继续。
        // -----------------------------------------------------------------
        TrapCause::TimerInterrupt => {
            // 必须是 now + interval, 不能传 interval (传相对量症状是
            // "前几次正常然后突然不再抢占")。走 timer::timer_reschedule,
            // kernel 侧的定时器接口 (间隔多少由它回答)。
            timer::timer_reschedule();

            // 记账: 这也是本阶段唯一的"中断真的在发生"的证据 (时钟中断
            // 除了重装闹钟什么也不做)。忘了调它, 屏幕上就是一个 tick
            // 都没有。返回值这里用不上 (lab-6 前没有调度器)。
            timer::timer_tick();
        }

        // -----------------------------------------------------------------
        // 外部设备中断: 从 PLIC 认领并处理。
        // -----------------------------------------------------------------
        TrapCause::ExternalInterrupt => {
            // 不能忽略: PLIC 会保持中断 pending, 导致无限重复卡死
            // 在这里, 所以必须 claim + complete。
            handle_external();
        }

        // -----------------------------------------------------------------
        // 核间中断: 目前只用于"叫醒其他核"。
        // -----------------------------------------------------------------
        TrapCause::SoftwareInterrupt => {
            // 软件中断是一次性的, 收到即清, 无需 complete; 当前无事可做。
        }

        // -----------------------------------------------------------------
        // 系统调用
        // -----------------------------------------------------------------
        TrapCause::SyscallFromUser => {
            // 推进返回地址: 陷入时返回地址指向 ecall 那条指令本身, 不
            // 推进则返回后重执行同一 ecall —— 系统调用无限重复 (症状因
            // 调用而异: write 刷屏 / exit 像死循环 / 有的"看起来能用")。
            // 注意只有系统调用才推进: 缺页等异常不能推进 (按需分页的
            // 处理是修好条件后重执行那条指令)。
            arch_trap::advance_return_address_for_syscall(tf);

            // 交给分发器, 并把已编码的 isize 写回 trapframe 的 a0 (用户
            // 程序 ecall 后从 a0 读返回值; 返回路径会用 trapframe 覆盖
            // 所有寄存器)。未实现的调用由 dispatch 内部返回 NoSys, 让
            // 学生能看出哪些调用真的能用。
            let ret = crate::syscall::dispatch(tf);
            tf.set_a0(ret as usize);
        }

        // -----------------------------------------------------------------
        // 内核自己执行 ecall 出错
        // -----------------------------------------------------------------
        TrapCause::SyscallFromKernel => {
            // 内核只在 SBI 调用时会 ecall。走到这里说明固件拒绝请求
            // (扩展号/功能号不被支持), 不是用户程序的错。
            report_and_park(tf, "SBI call rejected by firmware (bad ext/fid?)");
        }

        // -----------------------------------------------------------------
        // 缺页 (按需分页、COW 等在 lab-3/lab-6 实现)
        // -----------------------------------------------------------------
        TrapCause::InstructionPageFault | TrapCause::LoadPageFault | TrapCause::StorePageFault => {
            // 当前阶段没有按需分页, 缺页一定是真错误。打印 stval (出错
            // 地址) 是调试缺页最有用的信息。
            report_and_park(tf, "page fault (demand paging not implemented)");
        }

        // -----------------------------------------------------------------
        // 其他异常: 都是错误
        // -----------------------------------------------------------------
        TrapCause::IllegalInstruction => {
            report_and_park(tf, "illegal instruction");
        }
        TrapCause::InstructionMisaligned => {
            report_and_park(tf, "instruction address misaligned");
        }
        TrapCause::UnknownException(_) | TrapCause::UnknownInterrupt(_) => {
            report_and_park(tf, "unknown trap cause");
        }
    }

    // ---- 返回前: 设置 sret 的目标特权级 ----
    // 不能省、顺序不能错。用哪一版取决于目标特权级: 当前阶段 trap 都
    // 来自内核态, 但接入用户程序后 (lab-4) SyscallFromUser 分支改用
    // build_user_return_status。现在写这一行是为了让**任何**返回都是
    // 确定的: 硬件陷入时把 SPP 设为 1, 若不显式处理, 将来某次来自用户
    // 态的 trap 会把 SPP 设成 0 并残留, 下一次内核态 sret 错误返回 U 态。
    let saved_status = tf.status;
    tf.status = if was_from_user(cause) {
        arch_trap::build_user_return_status(saved_status)
    } else {
        arch_trap::build_kernel_return_status(saved_status)
    };
}

/// 判断这次 trap 是否来自用户态。
///
/// 依据 scause 而非 sstatus.SPP: SPP 在进入时被硬件设为原特权级, 但它
/// 可变 (处理中途修改 sstatus 会让它不再反映真实来源), 而 scause 在
/// 处理过程中不变。具体: ecall from U 一定是用户态; 缺页/非法指令可能
/// 来自两者, 此时才看 SPP。当前无用户程序, 但仍按正确规则判断, 让
/// lab-4 接入时不用改这里。
fn was_from_user(cause: TrapCause) -> bool {
    match cause {
        TrapCause::SyscallFromUser => true,
        TrapCause::SyscallFromKernel => false,
        // 对异常和中断, 用"陷入前的特权级"判断来源。
        _ => arch_trap::came_from_user_mode(),
    }
}

/// 处理外部设备中断。
///
/// 当前阶段从 platform 常量重新构造一个 Plic, 完成 claim + complete
/// 的完整流程 —— 不 complete 则中断无限重复、内核卡死且无输出提示。
fn handle_external() { }

/// 打印一份诊断信息并停车。
///
/// 不用 `panic!`: panic 处理看不到 trapframe, 而 trapframe 里有
/// scause/sepc/stval 及全部 31 个寄存器 —— 调试 trap 类问题最直接的信息。
fn report_and_park(tf: &TrapFrame, what: &str) -> ! {
    // 一次取全 trap 现场 (分开读会拼出一个假的现场, 见 hal 的 last_fault_info)。
    let fault = arch_trap::last_fault_info();

    console::with_lock(|| {
        oslab_hal::putchar::puts("\n");
        oslab_hal::putchar::puts("================ TRAP ================\n");
        oslab_hal::putchar::puts("  what  : ");
        oslab_hal::putchar::puts(what);
        oslab_hal::putchar::puts("\n");
        oslab_hal::putchar::puts("  cause : ");
        oslab_hal::putchar::puts(fault.cause_name);
        oslab_hal::putchar::puts(if fault.is_interrupt {
            " (interrupt)"
        } else {
            " (exception)"
        });
        oslab_hal::putchar::puts("\n");
        oslab_hal::putchar::puts("  raw   : ");
        console::print_hex(fault.cause_raw);
        oslab_hal::putchar::puts("   (code ");
        console::print_dec(arch_trap::cause_code(fault.cause_raw));
        oslab_hal::putchar::puts(")\n");
        oslab_hal::putchar::puts("  pc    : ");
        console::print_hex(fault.pc);
        oslab_hal::putchar::puts("   <-- 出错指令的地址\n");
        oslab_hal::putchar::puts("  addr  : ");
        console::print_hex(fault.address);
        oslab_hal::putchar::puts("   <-- 缺页地址 / 非法指令编码 (若适用)\n");
        oslab_hal::putchar::puts("  hartid: ");
        console::print_dec(oslab_hal::arch::cpu::hartid());
        oslab_hal::putchar::puts("\n");
        oslab_hal::putchar::puts("--------------------------------------\n");
        oslab_hal::putchar::puts("  pc 里那条指令可以用 objdump 查:\n");
        // 工具名来自 arch 层 (跟着架构走, 写死会误导换架构的学生)。
        oslab_hal::putchar::puts("    ");
        oslab_hal::putchar::puts(oslab_hal::arch::OBJDUMP);
        oslab_hal::putchar::puts(" -d <kernel.elf> | grep -A5 <pc 附近>\n");
        oslab_hal::putchar::puts("======================================\n");
        // 打印几个关键寄存器, 不全打 (完整 dump 会淹没真正重要的信息,
        // 需要全部寄存器时用 gdb)。
        oslab_hal::putchar::puts("  ra=");
        console::print_hex(tf.ra());
        oslab_hal::putchar::puts("  sp=");
        console::print_hex(tf.sp());
        oslab_hal::putchar::puts("  a0=");
        console::print_hex(tf.a0());
        oslab_hal::putchar::puts("  a7=");
        console::print_hex(tf.a7());
        oslab_hal::putchar::puts("\n");
    });

    oslab_hal::arch::time::park_current_hart()
}