//! 每个 hart 的时钟中断: 负责"什么时候装闹钟、装多少、记几次账"。
//! 真正读写时间 / 使能中断 / 平台间隔 都交给 hal 层。

use core::sync::atomic::{AtomicU64, Ordering};

use oslab_hal::arch;

use crate::console;

// 每个 hart 的 tick 账本: 记录本 hart 收到的时钟中断次数。
// per-hart `static`, 而不是全局计数器 —— 否则会掩盖"某个核根本没
// 收到中断"的事实。只用 `Relaxed`: 它只用于累加和打印, 没有同步语义。
static TICKS: AtomicU64 = AtomicU64::new(0);

/// 为当前 hart 装上第一次时钟中断。
///
/// 每个跑内核的 hart 都要各调一次。这里只装闹钟, 不使能中断:
/// 装闹钟与放行中断是两个独立决定, 调用方需要"先准备好再放行"。
pub fn timer_create() { }

/// 读当前时钟的 tick 数 (RISC-V 上即 `rdtime`)。
///
/// 单位是"平台时钟 tick", 不是秒 —— 换算前先经过 [`timer_interval`]。
/// 比较两个时刻用 `wrapping_sub`, 避免计数器回绕时触发溢出检查。
#[inline]
pub fn timer_get_ticks() -> u64 {
    arch::time::read_ticks()
}

/// 两次时钟中断的间隔, 单位是 tick。
///
/// 来自平台常量 (QEMU 10 MHz / VF2 4 MHz), 不在这里写死 ——
/// 换板子只需改平台文件一行。
#[inline]
pub fn timer_interval() -> u64 {
    arch::cpu::platform().timer_interval as u64
}

/// 处理完一次时钟中断后装下一次闹钟, 返回新设置的绝对时刻。
///
/// 必须用 `now + interval`, 而不是把 interval 当绝对时刻 ——
/// 前者不至于漂移到过去, 后者会在某个时刻被甩到过去造成中断风暴
/// 或"突然不再抢占"。加法在 hal 内部完成。
#[inline]
pub fn timer_reschedule() -> u64 {
    arch::time::set_next_deadline(timer_interval())
}

/// 记一次时钟中断, 返回累计的 tick 数 (含这一次)。
///
/// 时钟中断可能打断持锁的临界区, 所以这里不加锁、不分配、不打印。
/// lab-5 之后这里会调用 `scheduler::tick()`。
#[inline]
pub fn timer_tick() -> u64 { 0 }

/// 本 hart 自启动以来的时钟中断次数。
///
/// 唯一用途是验证时钟中断真的在发生: 在 idle 循环里打印它,
/// 数字不涨就说明中断没来。
#[inline]
pub fn timer_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// 打印当前 hart 的 tick 计数 (一行, 带 hartid)。
///
/// 计数器是 per-hart 的, 不带 hartid 的话多个核的输出会混在一起
/// 无法解释。用 `with_lock` 保证一整行不被别的核插进来。
pub fn timer_print_ticks() {
    console::with_lock(|| {
        oslab_hal::putchar::puts("[oslab-rs] hart ");
        console::print_dec(arch::cpu::hartid());
        print_ticks_body();
    });
}

/// 每 `period` 次 tick 打印一行, 给 idle 循环用的周期性自检。
///
/// 一行里的 t 与 interval 可互相校对: 相邻两行 t 之差应约等于
/// `period * interval`, 差了说明中断在风暴式触发或已停止。
/// `period == 0` 时什么都不做 (避免除零)。
pub fn timer_maybe_report(period: u64) {
    if period == 0 {
        return;
    }
    let n = timer_ticks();
    if n == 0 || n % period != 0 {
        return;
    }
    console::with_lock(|| {
        oslab_hal::putchar::puts("[oslab-rs] hart ");
        console::print_dec(arch::cpu::hartid());
        print_ticks_body();
    });
}

// 两个打印函数共用的输出体。私有函数, 用 `//` 即可。
fn print_ticks_body() {
    oslab_hal::putchar::puts(": ");
    console::print_dec(timer_ticks() as usize);
    oslab_hal::putchar::puts(" ticks (t=");
    console::print_dec(timer_get_ticks() as usize);
    oslab_hal::putchar::puts(", interval=");
    console::print_dec(timer_interval() as usize);
    oslab_hal::putchar::puts(")\n");
}

// 编译期自检: 平台把 timer_interval 抄成 0 会中断风暴, 所以编译期挡掉。
const _: () = {
    // 间隔必须是正数。
    assert!(interval_is_positive());

    // 间隔不能大到离谱 (2^32 tick 已是"400 秒一次", 不算时钟中断)。
    assert!(interval_is_sane());
};

/// 平台定时器间隔是否为正 —— 供上面编译期的断言使用。
///
/// 需要是 `const`, 于是走 `platform::PLATFORM` 而非运行期的
/// `arch::cpu::platform()`。两条断言因此在编译期求值, 抄错直接编译失败。
pub const fn interval_is_positive() -> bool {
    oslab_hal::platform::PLATFORM.timer_interval > 0
}

/// 平台定时器间隔是否在合理量级内。
pub const fn interval_is_sane() -> bool {
    oslab_hal::platform::PLATFORM.timer_interval <= (1u64 << 32) as usize
}