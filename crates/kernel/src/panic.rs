//! panic 处理。裸机上 panic 往往发生在内存耗尽、栈溢出、设备不响应
//! 的时刻, 所以遵守三条纪律: 不分配 (无堆)、不取锁 (输出先 try_lock,
//! 拿不到就放弃加锁直接写, 交错远好于打不出)、不返回 (停车等死)。
//! trap 处理里的 `report_and_park` 走独立路径, 不经过这里 (trap 有
//! trapframe, panic 看不到它, 混在一起会丢信息)。

use core::panic::PanicInfo;

use oslab_hal::arch;

use crate::console;

/// panic 处理器 (`#[panic_handler]` 是 freestanding 二进制必须有的符号)。
///
/// 用 `-> !`: panic 不返回, 让"panic 后继续执行"在类型层面不可能。
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // 先关中断: 若 panic 与中断处理有关, 让中断继续来会把输出搅乱。
    arch::irq::disable();

    oslab_hal::putchar::puts("\n");
    oslab_hal::putchar::puts("!!!!!!!! KERNEL PANIC !!!!!!!!\n");
    oslab_hal::putchar::puts("  hartid : ");
    console::print_dec(arch::cpu::hartid());
    oslab_hal::putchar::puts("\n");

    // 位置信息可能是 Option: 手写 panic! 一定带, 编译期生成的检查则不一定。
    match info.location() {
        Some(loc) => {
            oslab_hal::putchar::puts("  at     : ");
            oslab_hal::putchar::puts(loc.file());
            oslab_hal::putchar::puts(":");
            console::print_dec(loc.line() as usize);
            oslab_hal::putchar::puts(":");
            console::print_dec(loc.column() as usize);
            oslab_hal::putchar::puts("\n");
        }
        None => {
            oslab_hal::putchar::puts("  at     : <unknown location>\n");
        }
    }

    // 用 write_fmt 而非 format! —— 前者无分配, 后者需要 String。
    oslab_hal::putchar::puts("  message: ");
    let _ = crate::console::write_fmt(format_args!("{}", info.message()));
    oslab_hal::putchar::puts("\n");

    oslab_hal::putchar::puts("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n");

    // 报告上次 trap 的原因, 若 panic 由 trap 路径引起可立刻看出是哪一类。
    let fault = arch::trap::last_fault_info();
    oslab_hal::putchar::puts("  cause  : ");
    oslab_hal::putchar::puts(fault.cause_name);
    oslab_hal::putchar::puts(" (raw ");
    console::print_hex(fault.cause_raw);
    oslab_hal::putchar::puts(")\n");
    oslab_hal::putchar::puts("  pc     : ");
    console::print_hex(fault.pc);
    oslab_hal::putchar::puts("  addr : ");
    console::print_hex(fault.address);
    oslab_hal::putchar::puts("\n");

    arch::time::park_current_hart()
}