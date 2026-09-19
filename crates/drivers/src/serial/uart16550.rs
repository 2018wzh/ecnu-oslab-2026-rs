//! 16550 兼容 UART 驱动。
//!
//! QEMU virt 和 VisionFive2 的地址相同但输入时钟不同, 分频计算必须用
//! `platform.uart0_clock` —— 本文件不出现任何具体地址或时钟数字, 全部
//! 来自 `&Platform`, 所以同一份源码在两个平台都正确。
//!
//! 注意偏移 0/1 的双重身份: 是"数据寄存器"还是"分频寄存器"由 LCR 的
//! DLAB 位决定, 所以初始化必须严格按顺序 (先开 DLAB 写分频, 再关 DLAB
//! 设数据格式), 否则得到波特率错误的串口 (输出乱码)。

use oslab_hal::platform::Platform;

use crate::mmio::Mmio;

// ---------------------------------------------------------------------------
// 寄存器偏移
// ---------------------------------------------------------------------------

/// 接收数据 (读) / 发送数据 (写)。
const REG_RHR_THR: usize = 0;
/// 中断使能 (写) / 分频低字节 (DLAB=1 时)。
const REG_IER_DLL: usize = 1;
/// FIFO 控制 (写) / 中断识别 (读)。
const REG_FCR_IIR: usize = 2;
/// 线路控制。
const REG_LCR: usize = 3;
/// 调制解调器控制。
const REG_MCR: usize = 4;
/// 线路状态。
const REG_LSR: usize = 5;
/// 分频高字节 (DLAB=1 时)。
const REG_DLM: usize = 1;

/// LSR: 接收缓冲区有数据。
const LSR_DATA_READY: u8 = 1 << 0;
/// LSR: 上一个字节有溢出错误。
const LSR_OVERRUN: u8 = 1 << 1;
/// LSR: 发送保持寄存器为空 (可以写下一个字节)。
const LSR_THR_EMPTY: u8 = 1 << 5;
/// LSR: 发送器完全空闲 (移位寄存器也空了)。
const LSR_TRANSMITTER_IDLE: u8 = 1 << 6;

/// LCR: 8 位数据。
const LCR_WORD_LEN_8: u8 = 0b11;
/// LCR: 1 位停止位。
const LCR_STOP_1: u8 = 0;
/// LCR: 无校验。
const LCR_PARITY_NONE: u8 = 0;
/// LCR: 分频锁存使能 (DLAB)。
const LCR_DLAB: u8 = 1 << 7;

/// FCR: 使能 FIFO。
const FCR_ENABLE: u8 = 1 << 0;
/// FCR: 清空接收 FIFO。
const FCR_CLEAR_RX: u8 = 1 << 1;
/// FCR: 清空发送 FIFO。
const FCR_CLEAR_TX: u8 = 1 << 2;
/// FCR: 触发阈值 14 字节。
const FCR_TRIGGER_14: u8 = 0b11 << 6;

/// 目标波特率。两个平台都是 115200 8N1 —— 这是嵌入式世界的通用默认值。
pub const BAUD_RATE: u32 = 115_200;

/// UART 操作可能返回的错误。
///
/// 用枚举区分不同错误原因, 每种的处理方法不同, 错误信息才有用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartError {
    /// 平台常量里的输入时钟是 0 —— 一定是抄漏了。
    ZeroClock,
    /// 波特率是 0。
    ZeroBaud,
    /// 算出来的分频系数超过 16 位 (65535), 无法表示。
    DivisorOverflow,
    /// 初始化后的自检失败 (回读 LCR 不一致)。
    SelfTestFailed,
}

/// 16550 UART 驱动。
pub struct Uart16550 {
    /// 寄存器窗口。存 `Mmio` 而非 `&mut Mmio` 也不加锁: `putc` 可在任何
    /// 上下文 (含 panic/trap) 被调用, 加锁会死锁。代价是多核打印字符可能
    /// 交错, 比死锁好; 整行原子输出由 kernel 打印层加自旋锁解决。
    regs: Mmio,
    /// 该 UART 的输入时钟 (Hz), 从 platform 取。
    clock: u32,
    /// 算出来的分频系数, 保留供调试打印。
    divisor: u16,
    /// 中断号, 保留供 PLIC 初始化使用。
    irq: u32,
}

impl Uart16550 {
    /// 初始化 UART 并返回驱动实例。
    ///
    /// 参数是 `&Platform` 而非从全局读, 让依赖关系在类型签名上可见。
    pub fn init(plat: &Platform) -> Self {
        let mut uart = Self {
            // SAFETY: `plat.uart0_base` 来自 platform 层的编译期常量,
            // 并已由编译期断言检查过 4 KiB 对齐。映射由内核页表负责 ——
            // 在分页开启之前, 恒等映射意味着物理地址可直接访问。
            regs: unsafe { Mmio::new(plat.uart0_base) },
            clock: plat.uart0_clock,
            divisor: 0,
            irq: plat.uart0_irq,
        };
        // 初始化失败不使用 panic: 这个函数在启动早期被调用,
        // 而那时 panic 处理可能还没准备好。返回一个"尽力而为"的
        // 实例, 由 `probe()` 让调用者判断。
        let _ = uart.configure();
        uart
    }

    /// 配置波特率与数据格式。
    ///
    /// 顺序不可换: 1) 关中断 (本内核用轮询) 2) 开 DLAB 写分频后关 DLAB
    /// (先写分频会写进 THR 成一串乱码) 3) 设数据格式 8N1
    /// 4) 清空并使能 FIFO (须在格式设置之后)。
    pub fn configure(&mut self) -> Result<(), UartError> {
        if self.clock == 0 {
            return Err(UartError::ZeroClock);
        }
        if BAUD_RATE == 0 {
            return Err(UartError::ZeroBaud);
        }

        // 分频系数 = 输入时钟 / (16 * 波特率), 分母 16 是 16550 的固定
        // 采样设计。四舍五入 (`(clock + 8*baud)/denom`) 比截断更准。
        // 算错或波特率抄错会导致串口乱码 (内核还在跑, 只是时序错)。
        let denom = 16u32 * BAUD_RATE;
        let divisor = (self.clock + denom / 2) / denom;
        if divisor == 0 || divisor > 0xffff {
            return Err(UartError::DivisorOverflow);
        }
        self.divisor = divisor as u16;

        // SAFETY: 下面所有访问都落在 16550 的寄存器窗口 (基址 +0..+7)
        // 之内, 而该窗口由 platform 常量给出且已映射。
        unsafe {
            // 1. 关中断。
            self.regs.write_u8(REG_IER_DLL, 0x00);

            // 2. 开 DLAB -> 写分频 -> 关 DLAB。
            self.regs.write_u8(REG_LCR, LCR_DLAB);
            self.regs.write_u8(REG_RHR_THR, (self.divisor & 0xff) as u8);
            self.regs.write_u8(REG_DLM, (self.divisor >> 8) as u8);
            self.regs
                .write_u8(REG_LCR, LCR_WORD_LEN_8 | LCR_STOP_1 | LCR_PARITY_NONE);

            // 3. FIFO: 清空并使能, 阈值设成 14 字节。
            self.regs.write_u8(
                REG_FCR_IIR,
                FCR_ENABLE | FCR_CLEAR_RX | FCR_CLEAR_TX | FCR_TRIGGER_14,
            );

            // 4. 自检: 回读 LCR 确认生效。若 UART 地址错 (指向无设备空间),
            // 读回值不一致 (QEMU 上未映射地址读回 0), 把它变成明确的错误
            // 而不是"输出乱码/无输出"。
            let lcr = self.regs.read_u8(REG_LCR);
            let expected = LCR_WORD_LEN_8 | LCR_STOP_1 | LCR_PARITY_NONE;
            if lcr != expected {
                return Err(UartError::SelfTestFailed);
            }
        }

        Ok(())
    }

    /// 主动探测: 确认这块 UART 真的在那里。
    ///
    /// 返回 `true` 表示自检通过。启动流程会打印这个结果 —— 它是
    /// "platform 常量正确性"的可见证据。
    pub fn probe(&self) -> bool {
        // SAFETY: 偏移 3 (LCR) 在寄存器窗口内。
        let lcr = unsafe { self.regs.read_u8(REG_LCR) };
        let expected = LCR_WORD_LEN_8 | LCR_STOP_1 | LCR_PARITY_NONE;
        lcr == expected
    }

    /// 分频系数 (调试用)。
    pub fn divisor(&self) -> u16 {
        self.divisor
    }

    /// 中断号。
    pub fn irq(&self) -> u32 {
        self.irq
    }

    /// 基地址。
    pub fn base(&self) -> usize {
        self.regs.base()
    }

    /// 输出一个字节 (轮询, 阻塞直到可以写)。
    pub fn putc(&self, c: u8) {
        // SAFETY: 两次访问都在寄存器窗口内。
        unsafe {
            // 等发送保持寄存器为空: 不等会覆盖上一个字节 (丢字符)。
            // 要超时: 若 UART 不存在或时钟停, THR_EMPTY 永不置位, 没超时
            // 就是死循环。检查 THR_EMPTY **或** TRANSMITTER_IDLE 提升兼容性。
            let mut spins: u32 = 0;
            loop {
                let lsr = self.regs.read_u8(REG_LSR);
                if lsr & (LSR_THR_EMPTY | LSR_TRANSMITTER_IDLE) != 0 {
                    break;
                }
                spins = spins.wrapping_add(1);
                if spins == u32::MAX {
                    // 超时: 放弃这个字节, 而不是永远卡住 (打印路径的死循环
                    // 会让整个内核失去诊断能力)。
                    return;
                }
            }
            self.regs.write_u8(REG_RHR_THR, c);
        }
    }

    /// 底层写函数, 供注册到 `hal::putchar` 使用。
    ///
    /// 必须是 `fn` (无 `self`) 才能转成函数指针。它从 platform 常量重新
    /// 构造轻量访问器, 而非访问全局单例: 零成本、不需要 `static mut`,
    /// panic 路径不依赖可能未初始化的全局状态。
    pub fn putc_raw(c: u8) {
        let uart = Self {
            // SAFETY: 同 `init`。
            regs: unsafe { Mmio::new(oslab_hal::platform::PLATFORM.uart0_base) },
            clock: oslab_hal::platform::PLATFORM.uart0_clock,
            divisor: 0,
            irq: oslab_hal::platform::PLATFORM.uart0_irq,
        };
        // 串口终端的约定: 换行要发 \r\n。这个转换放在驱动层而非打印层,
        // 因为它是**终端/线路**的约定, 不是格式化的职责。
        if c == b'\n' {
            uart.putc(b'\r');
        }
        uart.putc(c);
    }

    /// 非阻塞读一个字节。
    pub fn getc(&self) -> Option<u8> {
        // SAFETY: 两次访问都在寄存器窗口内。
        unsafe {
            let lsr = self.regs.read_u8(REG_LSR);
            if lsr & LSR_DATA_READY != 0 {
                Some(self.regs.read_u8(REG_RHR_THR))
            } else {
                None
            }
        }
    }

    /// 阻塞读一个字节 (中断关闭时会一直自旋)。
    pub fn getc_blocking(&self) -> u8 {
        loop {
            if let Some(c) = self.getc() {
                return c;
            }
            core::hint::spin_loop();
        }
    }

    /// 清空接收缓冲区里所有待处理的字节, 返回清掉的个数。
    pub fn drain(&self) -> usize {
        let mut n = 0;
        while self.getc().is_some() {
            n += 1;
        }
        n
    }

    /// 是否有接收溢出错误。
    pub fn has_overrun(&self) -> bool {
        // SAFETY: 偏移 5 (LSR) 在寄存器窗口内。
        let lsr = unsafe { self.regs.read_u8(REG_LSR) };
        lsr & LSR_OVERRUN != 0
    }
}

// ===========================================================================
// 编译期自检
// ===========================================================================
const _: () = {
    // 16550 的寄存器窗口只有 8 个字节, 所有偏移必须落在这个范围内。
    // 这条断言防的是"抄寄存器表时把偏移抄成 8 或更大"。
    assert!(REG_RHR_THR < 8);
    assert!(REG_IER_DLL < 8);
    assert!(REG_FCR_IIR < 8);
    assert!(REG_LCR < 8);
    assert!(REG_MCR < 8);
    assert!(REG_LSR < 8);
    // 偏移 1 的双重身份: IER 与 DLM 是同一个地址。
    // 这不是笔误, 而是 16550 的设计 (由 LCR.DLAB 选择)。
    assert!(REG_IER_DLL == REG_DLM);
};
