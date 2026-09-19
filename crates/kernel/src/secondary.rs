//! 从核 (secondary hart) 的启动路径。
//!
//! 启动核被固件复位时跳进汇编 `_entry`; 从核则被 `start_others` 经 SBI
//! HART_START 在运行期叫醒, 入口就是传给固件的地址 —— 没有汇编铺垫。
//! 刚被唤醒时 sp/tp/satp 都未定义, 三个都会静默失败 (不建栈 -> Rust 入口
//! 把 ra 存到随机地址; 不设 tp -> cpu_id() 返回 None 而停车; 不清 satp ->
//! 经过未知页表)。所以从核也需要汇编引导段 (在 hal 层, 即 SECONDARY_ENTRY),
//! 其栈编号规则必须与启动核 `_entry` 一致 (共用同一个 boot_stacks 数组)。

use crate::console;
use oslab_hal::arch;

// 从核的汇编入口 (hal 层提供): 从 a0 拿 hartid、按与启动核相同规则算
// 栈顶进 sp、hartid 写进 tp、清 satp、关中断, 再 call secondary_main。
// 这些寄存器里的 CSR 名与指令属于 arch 层, 不在 kernel; 常量直接来自
// platform 模块 (global_asm! 的 const 操作数), 没有第二份定义。

/// 从核的 Rust 主函数。
///
/// 比启动核少做很多, 且"少"是刻意的: 不清 `.bss` (启动核已清过, 再清
/// 会抹掉全局状态)、不打印完整横幅 (输出会交错)、不做全局设备初始化
/// (幂等但重复做只与启动核竞争)。做: 装自己的 stvec (per-hart CSR,
/// 每个核必须各装一次)、打印一行在线、使能核间中断、进 idle。
#[unsafe(no_mangle)]
pub extern "C" fn secondary_main() -> ! {
    let hartid = arch::cpu::hartid();

    // 校验 hartid (汇编查过一次, 这里用 platform 层同源再查一次):
    // 将来若有人删掉汇编里的检查, 这里仍拦住"在非法 hart 上运行"。
    let Some(cpuid) = arch::cpu::cpu_id() else {
        oslab_hal::putchar::puts("[oslab-rs] secondary hart ");
        console::print_dec(hartid);
        oslab_hal::putchar::puts(" is outside the platform hart range; parking\n");
        arch::time::park_current_hart()
    };

    // 装 trap 向量。每个 hart 的 stvec 独立, 从核带 stvec=0 收中断会
    // 跳到地址 0 执行垃圾。
    //
    // SAFETY: TRAP_ENTRY 首条指令 (csrrw sp, sscratch, sp) 不依赖已有
    // 寄存器, 满足 trap 入口最低要求。

    // 还没有真 trap 系统时的兜底: 收到中断明确卡住, 不跳地址 0。这是
    // 可接受的 —— 此阶段本就不该有中断 (外部中断源与定时器未配置)。
    //
    // SAFETY: park_forever 不返回、不用任何寄存器, 满足 trap 入口最低要求。
    unsafe {
        arch::trap::install_vector(arch::trap::park_forever);
    }

    // 打印一行"在线": 这是"从核真跑起来了"的唯一证据 —— 启动流程报
    // 的 "N now online" 只是固件接受了请求, 不等于从核真的执行到它。
    // 多核写同一串口字符可能交错, 可接受的代价 (能交错远好于等锁死锁)。
    oslab_hal::putchar::puts("[oslab-rs] hart ");
    console::print_dec(hartid);
    oslab_hal::putchar::puts(" (cpu ");
    console::print_dec(cpuid);
    oslab_hal::putchar::puts(") online, sp=");
    console::print_hex(read_sp());
    oslab_hal::putchar::puts("\n");

    // 使能核间中断 (为将来的 TLB 刷新广播准备)。
    arch::irq::enable_source(arch::trap::source::SOFTWARE);

    idle_loop(cpuid, hartid)
}

/// 从核的 idle 循环。
///
/// 与启动核的 [`crate::idle_loop`] 分开写, 因为崩溃报告各自带不同
/// 的识别信息, 分成两个函数代码路径更易读。
fn idle_loop(cpuid: usize, hartid: usize) -> ! {
    let mut ticks: usize = 0;
    loop {
        // SAFETY: `wfi` 只影响调度, 不改变任何状态。
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }
        ticks = ticks.wrapping_add(1);

        // 每 100000 轮查一次栈 canary (理由见 crate::idle_loop)。
        if ticks % 100_000 == 0 && !arch::boot::check_stack_canary(cpuid) {
            oslab_hal::putchar::puts("\n[oslab-rs] FATAL: kernel stack overflow on hart ");
            console::print_dec(hartid);
            oslab_hal::putchar::puts(" (cpu ");
            console::print_dec(cpuid);
            oslab_hal::putchar::puts(")\n");
            crate::crash_report();
        }
    }
}

/// 读当前的 sp (调试用) —— 排查"从核用错栈槽"最直接的手段: 两个核
/// 若打印出相同的 sp, 说明栈索引算错了 (症状是随机内存破坏, 难反推)。
#[inline]
fn read_sp() -> usize {
    let v: usize;
    // SAFETY: 读 sp 没有副作用。
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}