//! `arch::smp` — 多核启动。
//!
//! 计算要启动哪些 hart (用平台 hart 区间, 不是 `0..ncpu`), 通过 SBI
//! HSM 逐个启动, 并检查每个的返回值。区间的重要性: 两个平台编号起点
//! 不同 (QEMU 0..1 / VF2 1..4); 写成 `0..ncpu` 会跳掉合法 hart 或尝试
//! 启动 U-Boot 占用监控核。不查返回值会让"某个核起不来"无日志。

use crate::arch::cpu;
use crate::arch::sbi;

/// 启动除当前 hart 之外的所有 hart, 返回启动成功的个数。
///
/// 入口地址由 [`secondary_entry_addr`] 提供 (kernel crate), 通过参数传入
/// 而非全局变量, 避免"谁在何时写"以及从核过早读到旧值的窗口。
pub fn start_others(secondary_entry: usize) -> usize {
    let plat = cpu::platform();
    let me = cpu::hartid();
    let mut started = 0;

    // 遍历**区间**, 不是 0..ncpu。见文件顶部说明。
    let mut hartid = plat.harts.min;
    while hartid <= plat.harts.max {
        if hartid != me {
            // opaque = 0: 从核自己能读 tp 之外的 hartid (固件在 a0 里给)。
            let ret = sbi::hart_start(hartid, secondary_entry, 0);
            if ret == 0 {
                started += 1;
            }
            // 非 0 不 panic: 少一个核不应让整个系统起不来, 调用者会打印实际数字。
        }
        hartid += 1;
    }

    // 给从核一点时间完成初始化, 让它们的输出在启动核后续输出之前出现。
    // 用有界忙等而非原子计数器等从核报到, 避免"从核卡住 -> 启动核永远等"。
    for _ in 0..20_000_000 {
        core::hint::spin_loop();
    }

    started
}

/// 报告每个 hart 的在线状态 (调试用), 实现在 kernel crate。
pub fn report_hart_status() {
    // 这个函数只打印, 由调用者决定是否调用。
}

/// 查询所有 hart 的在线状态, 返回 `(hartid, status)` 列表的长度。
pub fn count_online_harts() -> usize {
    let plat = cpu::platform();
    let mut n = 0;
    let mut hartid = plat.harts.min;
    while hartid <= plat.harts.max {
        // 必须判 `Some(Started)` 而非"值等于 0": 查询失败的返回值也是 0,
        // 会把"不存在的 hart"算成在线 (见 `sbi::hart_status` 的说明)。
        if sbi::hart_status(hartid) == Some(sbi::HartState::Started) {
            n += 1;
        }
        hartid += 1;
    }
    n
}
