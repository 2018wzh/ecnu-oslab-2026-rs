//! 通过 SBI (OpenSBI 固件) 向固件请求服务: 打印、定时器、启动 hart、复位。
//!
//! 是固件接口层, 与 arch 层并列但职责不同 —— 内核只说"启动第 3 个 CPU",
//! 由这里的 SBI 封装去调用固件。
//!
//! SBI 调用约定 (与 Linux 相同): `a7` = 扩展 ID, `a6` = 功能 ID,
//! `a0-a5` = 参数, `ecall` 陷入 M-mode, 返回 `a0` = 错误码, `a1` = 返回值。
//!
//! 启动横幅先走 SBI 而非 UART, 是为分层验证: SBI 不依赖 platform 的
//! UART 地址, 只要固件能启动就一定能打印。

/// SBI 调用的返回值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SbiRet {
    /// 错误码。0 = 成功。
    pub error: isize,
    /// 返回值。
    pub value: isize,
}

impl SbiRet {
    /// 是否成功。
    pub const fn is_ok(&self) -> bool {
        self.error == 0
    }
}

// ---------------------------------------------------------------------------
// 扩展 ID
// ---------------------------------------------------------------------------

/// BASE 扩展: 查询 SBI 版本与可用扩展。
pub const EXT_BASE: usize = 0x10;
/// 旧版 (Legacy) 控制台扩展: 输出一个字符。
///
/// SBI v0.2 后标记为 legacy, 但所有固件 (含 QEMU 的 OpenSBI) 都实现;
/// 是"能打印"的最短路径, 最适合排查启动问题。
pub const EXT_LEGACY_PUTCHAR: usize = 0x01;
/// 旧版控制台扩展: 读一个字符 (无数据时返回 -1)。
pub const EXT_LEGACY_GETCHAR: usize = 0x02;

/// TIME 扩展 (定时器)。ASCII "TIME"。
pub const EXT_TIME: usize = 0x5449_4D45;
/// IPI 扩展 (核间中断)。ASCII "sPI"。
pub const EXT_IPI: usize = 0x0073_5049;
/// RFENCE 扩展 (远程 TLB 刷新)。ASCII "RFNC"。
pub const EXT_RFENCE: usize = 0x5246_4E43;
/// HSM 扩展 (hart 状态管理)。ASCII "HSM"。
pub const EXT_HSM: usize = 0x0048_534D;
/// SRST 扩展 (系统复位)。ASCII "SRST"。
pub const EXT_SRST: usize = 0x5352_5354;
/// DBCN 扩展 (调试控制台, 支持一次写多个字符)。ASCII "DBCN"。
pub const EXT_DBCN: usize = 0x4442_434E;

/// TIME 扩展: 设置下一次时钟中断的**绝对**时间。
pub const TIME_SET_TIMER: usize = 0;

/// IPI 扩展: 发送核间中断。功能 0。
pub const IPI_SEND: usize = 0;

/// HSM 报告的一个 hart 的状态。
///
/// 用具名枚举而非裸数字, 因为 `0` 同时是 "STARTED" 与 "调用成功" ——
/// 裸数字会把"查询失败"误读成"hart 已启动"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HartState {
    /// 正在运行 (包括在 M-mode 固件里)。
    Started,
    /// 已停止, 可以被 `hart_start` 启动。
    Stopped,
    /// 启动中 (尚未开始执行入口)。
    StartPending,
    /// 停止中。
    StopPending,
}

impl HartState {
    /// 把固件给的原始值翻译成枚举; 未知值返回 `None`, 不悄悄落回默认值。
    pub const fn from_raw(v: isize) -> Option<Self> {
        Some(match v {
            0 => HartState::Started,
            1 => HartState::Stopped,
            2 => HartState::StartPending,
            3 => HartState::StopPending,
            _ => return None,
        })
    }
}

/// HSM 扩展: 启动一个 hart。
pub const HSM_HART_START: usize = 0;
/// HSM 扩展: 停止当前 hart。
pub const HSM_HART_STOP: usize = 1;
/// HSM 扩展: 查询 hart 状态。
pub const HSM_HART_GET_STATUS: usize = 2;

/// SRST 扩展: 复位系统。
pub const SRST_RESET: usize = 0;
/// SRST 重置类型: 关机。
pub const SRST_TYPE_SHUTDOWN: usize = 0;
/// SRST 重置类型: 冷启动。
pub const SRST_TYPE_COLD_REBOOT: usize = 1;
/// SRST 重置原因: 无。
pub const SRST_REASON_NONE: usize = 0;

/// DBCN 扩展: 写多个字符。
pub const DBCN_CONSOLE_WRITE: usize = 0;
/// DBCN 扩展: 写一个字符。
pub const DBCN_CONSOLE_WRITE_BYTE: usize = 2;

/// 底层 `ecall` —— 整个 crate 里**唯一**执行 `ecall` 的地方。
///
/// 把唯一一条特权指令收在一处, 让"内核何时陷入固件"可被 `grep` 出来。
/// 它本身是安全的: `ecall` 陷入 M-mode 是受控操作, 固件会校验参数,
/// 非法参数只返回错误码。
#[inline]
fn ecall(eid: usize, fid: usize, a0: usize, a1: usize, a2: usize) -> SbiRet {
    let (err, val): (isize, isize);
    // SAFETY: `ecall` 是 RISC-V 的 S-mode -> M-mode 入口; 寄存器约束按
    // SBI 规范 (a0-a2 参数, a6 = FID, a7 = EID; a0/a1 用 inlateout)。
    unsafe {
        ::core::arch::asm!(
            "ecall",
            inlateout("a0") a0 => err,
            inlateout("a1") a1 => val,
            in("a2") a2,
            in("a6") fid,
            in("a7") eid,
            options(nostack),
        );
    }
    SbiRet {
        error: err,
        value: val,
    }
}

/// 向固件控制台输出一个字符 (阻塞直到写出)。
///
/// 优先用 legacy 扩展: 它从 SBI v0.1 起所有实现都保留, "一定能打印"
/// 比"打印得快"重要 (DBCN 更新但不是所有固件都实现)。
#[inline]
pub fn console_putchar(c: u8) {
    let r = ecall(EXT_LEGACY_PUTCHAR, 0, c as usize, 0, 0);
    if r.error != 0 {
        // 极端情况下固件移除了 legacy, 退回 DBCN。
        ecall(EXT_DBCN, DBCN_CONSOLE_WRITE_BYTE, c as usize, 0, 0);
    }
}

/// 从固件控制台读一个字符; 没有数据时返回 `None`。
#[inline]
pub fn console_getchar() -> Option<u8> {
    let r = ecall(EXT_LEGACY_GETCHAR, 0, 0, 0, 0);
    // legacy 约定: "暂无数据"的 -1 出现在 **error** 字段而非 value;
    // 值 0..=255 是有效字符。
    if r.error < 0 || r.error > 255 {
        None
    } else {
        Some(r.error as u8)
    }
}

/// 设置下一次时钟中断的**绝对**时间。
///
/// 参数是"`time` CSR 等于多少时中断", 不是"多久之后"。传相对量当 `time`
/// 超过它时会落在**过去**, 中断永不触发 (前几次正常, 然后不再被抢占)。
/// 正确写法是 `set_timer(read_time() + interval)`。
#[inline]
pub fn set_timer(stime_value: u64) {
    ecall(EXT_TIME, TIME_SET_TIMER, stime_value as usize, 0, 0);
}

/// 启动另一个 hart, 让它从 `start_addr` 开始执行。
///
/// `opaque` 作为 `a1` 传给目标; `a0` 由固件填成目标 hart 自己的 hartid。
/// 0 表示成功, **调用者必须检查返回值** —— 忽略错误的表现是"某个核
/// 永远起不来"却没有日志。
#[inline]
pub fn hart_start(hartid: usize, start_addr: usize, opaque: usize) -> isize {
    ecall(EXT_HSM, HSM_HART_START, hartid, start_addr, opaque).error
}

/// 停止当前 hart。一般不会返回。
#[inline]
pub fn hart_stop() {
    ecall(EXT_HSM, HSM_HART_STOP, 0, 0, 0);
}

/// 查询某个 hart 的状态。
#[inline]
pub fn hart_status(hartid: usize) -> Option<HartState> {
    let ret = ecall(EXT_HSM, HSM_HART_GET_STATUS, hartid, 0, 0);
    // 必须检查 error: `error != 0` 时 value 未定义, 而 OpenSBI 失败时
    // 把 value 留成 0 —— 恰好是 `STARTED`。只看 value 会把"不存在的
    // hart"误报成"已启动"。
    if ret.error != 0 {
        return None;
    }
    HartState::from_raw(ret.value)
}

/// 关闭整个系统 (QEMU 会因此退出)。
#[inline]
pub fn system_shutdown() {
    ecall(
        EXT_SRST,
        SRST_RESET,
        SRST_TYPE_SHUTDOWN,
        SRST_REASON_NONE,
        0,
    );
}

/// 查询 SBI 规范版本号, 返回 `(major, minor)`。
///
/// 启动横幅里打印它, 让内核跑在哪个固件之上可见 (固件版本决定扩展可用性)。
#[inline]
pub fn spec_version() -> (usize, usize) {
    let r = ecall(EXT_BASE, 0, 0, 0, 0);
    if r.error != 0 {
        return (0, 0);
    }
    let v = r.value as usize;
    (v >> 24, (v >> 16) & 0xff)
}

/// 发送核间中断。`hart_mask` 的每一位对应一个 hart id。
///
/// `pub(crate)`: 只有 [`arch::cpu::send_ipi`] 的语义化封装供上层使用,
/// 不让上层直接接触固件的编号约定。
#[inline]
pub(crate) fn send_ipi_raw(hart_mask: usize) {
    // "hart_mask_base" (SBI v0.2 起) 为 0, 表示位 0 对应 hart 0。
    // 本内核 hart 编号从 0 或 1 开始, 都在位图低 64 位内, 够用。
    ecall(EXT_IPI, IPI_SEND, hart_mask, 0, 0);
}

/// 用 SBI 控制台输出一个字符串 (仅启动早期使用)。
///
/// UART 驱动就绪后应改用 `oslab_hal::putchar`, 它走真正的串口硬件。
pub fn console_write_str(s: &str) {
    for b in s.bytes() {
        // 终端习惯: `\n` 要发成 `\r\n`, 否则光标只下移不回行首 (阶梯状输出)。
        if b == b'\n' {
            console_putchar(b'\r');
        }
        console_putchar(b);
    }
}
