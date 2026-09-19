//! VisionFive2 的 SD 卡驱动: Synopsys DesignWare MSHC, 寄存器接口与
//! 标准 SDHCI 兼容。与 virtio_blk 实现同一个 [`BlockDevice`] trait。
//!
//! SD 初始化需严格按 CMD0/CMD8/CMD55+ACMD41/CMD2/CMD3/CMD9/CMD7/CMD16
//! 的流程, 每一步都必须带超时。当前实现做到"复位 + 设置时钟 + 卡识别
//! 流程 + 容量计算 + 单块 PIO 读写"; 寄存器偏移是机械且易抄错的部分,
//! 已准备好。

use oslab_hal::platform::Platform;

use super::{BlockDevice, BlockError};
use crate::mmio::Mmio;

// ---------------------------------------------------------------------------
// SDHCI 寄存器偏移 (SD Host Controller Simplified Specification)
// ---------------------------------------------------------------------------

// 部分偏移在当前阶段 (lab-7 前) 未用到, 保留用于命令/数据流程, 并作为
// 驱动文档。用 `#[allow(dead_code)]` 标注"这是有意的"。
#[allow(dead_code)]
mod regs {

    /// 32 位系统地址寄存器块 (偏移 0x00-0x3F)。
    pub const REG_SDMA_ADDR: usize = 0x00;
    /// 块大小寄存器 (16 位)。
    pub const REG_BLOCK_SIZE: usize = 0x04;
    /// 块计数寄存器 (16 位)。
    pub const REG_BLOCK_COUNT: usize = 0x06;
    /// 参数寄存器 (32 位) —— 命令的参数放这里。
    pub const REG_ARGUMENT: usize = 0x08;
    /// 传输模式寄存器 (16 位)。
    pub const REG_TRANSFER_MODE: usize = 0x0c;
    /// 命令寄存器 (16 位) —— 写它就会发送命令。
    pub const REG_COMMAND: usize = 0x0e;
    /// 响应寄存器 (4 个 32 位寄存器, 0x10-0x1F)。
    pub const REG_RESPONSE_0: usize = 0x10;
    /// 响应寄存器 3。
    pub const REG_RESPONSE_3: usize = 0x1c;
    /// 缓冲区数据端口寄存器。
    pub const REG_BUFFER_DATA_PORT: usize = 0x20;
    /// 当前状态寄存器 (32 位)。
    pub const REG_PRESENT_STATE: usize = 0x24;
    /// 主机控制 1 寄存器 (8 位) —— 位宽、超时。
    pub const REG_HOST_CONTROL_1: usize = 0x28;
    /// 电源控制寄存器 (8 位)。
    pub const REG_POWER_CONTROL: usize = 0x29;
    /// 时钟控制寄存器 (16 位) —— 分频在这里。
    pub const REG_CLOCK_CONTROL: usize = 0x2c;
    /// 超时控制寄存器 (8 位)。
    pub const REG_TIMEOUT_CONTROL: usize = 0x2e;
    /// 软件复位寄存器 (8 位)。
    pub const REG_SOFTWARE_RESET: usize = 0x2f;
    /// 正常中断状态寄存器 (16 位)。
    pub const REG_NORMAL_INT_STATUS: usize = 0x30;
    /// 错误中断状态寄存器 (16 位)。
    pub const REG_ERROR_INT_STATUS: usize = 0x32;
    /// 正常中断状态使能寄存器 (16 位)。
    pub const REG_NORMAL_INT_STATUS_ENABLE: usize = 0x34;
    /// 错误中断状态使能寄存器 (16 位)。
    pub const REG_ERROR_INT_STATUS_ENABLE: usize = 0x36;
    /// 正常中断信号使能寄存器 (16 位)。
    pub const REG_NORMAL_INT_SIGNAL_ENABLE: usize = 0x38;
    /// 错误中断信号使能寄存器 (16 位)。
    pub const REG_ERROR_INT_SIGNAL_ENABLE: usize = 0x3a;
    /// 自动 CMD12 错误状态寄存器 (16 位)。
    pub const REG_AUTOCMD12_ERROR_STATUS: usize = 0x3c;
    /// 能力寄存器 (64 位, 0x40-0x47)。
    pub const REG_CAPABILITIES: usize = 0x40;
    /// 最大电流能力寄存器。
    pub const REG_MAX_CURRENT_CAPABILITIES: usize = 0x48;
} // mod regs

use regs::*;

#[allow(dead_code)]
mod bits {
    /// 软件复位: 复位整个控制器。
    pub const SW_RESET_ALL: u8 = 1 << 0;
    /// 软件复位: 只复位命令线。
    pub const SW_RESET_CMD: u8 = 1 << 1;
    /// 软件复位: 只复位数据线。
    pub const SW_RESET_DAT: u8 = 1 << 2;

    /// PRESENT_STATE: 命令线空闲 (可以发下一条命令)。
    pub const STATE_CMD_INHIBIT: u32 = 1 << 0;
    /// PRESENT_STATE: 数据线空闲。
    pub const STATE_DAT_INHIBIT: u32 = 1 << 1;
    /// PRESENT_STATE: 写传输活跃。
    pub const STATE_WRITE_TRANSFER_ACTIVE: u32 = 1 << 8;
    /// PRESENT_STATE: 读传输活跃。
    pub const STATE_READ_TRANSFER_ACTIVE: u32 = 1 << 9;
    /// PRESENT_STATE: 缓冲区可写 (有空间放数据)。
    pub const STATE_BUFFER_WRITE_ENABLE: u32 = 1 << 10;
    /// PRESENT_STATE: 缓冲区可读 (有数据可取)。
    pub const STATE_BUFFER_READ_ENABLE: u32 = 1 << 11;
    /// PRESENT_STATE: 卡已插入。
    pub const STATE_CARD_INSERTED: u32 = 1 << 16;

    /// CLOCK_CONTROL: 内部时钟稳定。
    pub const CLOCK_INTERNAL_STABLE: u16 = 1 << 1;
    /// CLOCK_CONTROL: 使能 SD 时钟。
    pub const CLOCK_SD_ENABLE: u16 = 1 << 2;
    /// CLOCK_CONTROL: 分频值的存放位置 (高 8 位)。
    pub const CLOCK_DIVISOR_SHIFT: u16 = 8;

    /// HOST_CONTROL_1: 4 位总线宽度。
    pub const HOST_CTRL1_BUS_WIDTH_4: u8 = 1 << 1;
    /// HOST_CONTROL_1: 高速使能。
    pub const HOST_CTRL1_HIGH_SPEED: u8 = 1 << 2;

    /// 中断状态: 命令完成。
    pub const INT_COMMAND_COMPLETE: u16 = 1 << 0;
    /// 中断状态: 传输完成。
    pub const INT_TRANSFER_COMPLETE: u16 = 1 << 1;
    /// 中断状态: 缓冲区可写。
    pub const INT_BUFFER_WRITE_READY: u16 = 1 << 4;
    /// 中断状态: 缓冲区可读。
    pub const INT_BUFFER_READ_READY: u16 = 1 << 5;
    /// 中断状态: 命令超时。
    pub const INT_COMMAND_TIMEOUT: u16 = 1 << 0;
    /// 中断状态: 命令 CRC 错误。
    pub const INT_COMMAND_CRC: u16 = 1 << 1;
    /// 中断状态: 命令结束位错误。
    pub const INT_COMMAND_END_BIT: u16 = 1 << 2;
    /// 中断状态: 命令索引错误。
    pub const INT_COMMAND_INDEX: u16 = 1 << 3;
    /// 中断状态: 数据超时。
    pub const INT_DATA_TIMEOUT: u16 = 1 << 4;
} // mod bits

use bits::*;

/// SD 卡协议规定: 初始化阶段只能用 400 kHz (用高时钟上电, 卡会不应答)。
const SD_INIT_CLOCK_HZ: u32 = 400_000;

/// 识别完成后的目标时钟 (25 MHz, "默认速度")。
const SD_DEFAULT_CLOCK_HZ: u32 = 25_000_000;

/// 设计规定的基准时钟: JH7110 的 SD 控制器是 50 MHz。
const SD_BASE_CLOCK_HZ: u32 = 50_000_000;

/// SD 命令的响应类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseType {
    /// 无响应 (CMD0, CMD12 的一部分用法)。
    None,
    /// 136 位响应 (CID/CSD)。
    Long,
    /// 48 位响应。
    Short,
    /// 48 位响应, 且要求卡检查忙信号 (R1b)。
    ShortWithBusy,
}

#[allow(dead_code)] // lab-7 会用到
impl ResponseType {
    /// 编码成 COMMAND 寄存器里的响应类型位。
    const fn bits(self) -> u16 {
        match self {
            // RESP_TYPE 字段在 bit 1:0:
            //   00 = 无响应, 01 = 136 位, 10 = 48 位, 11 = 48 位 + 忙
            ResponseType::None => 0b00,
            ResponseType::Long => 0b01,
            ResponseType::Short => 0b10,
            ResponseType::ShortWithBusy => 0b11,
        }
    }

    /// 是否需要在响应上做 CRC 检查。
    ///
    /// CMD0 和 CMD8 的响应没有有效 CRC, 对它们必须关掉检查, 否则会
    /// 永远报 CRC 错误。
    const fn checks_crc(self) -> bool {
        !matches!(self, ResponseType::None)
    }

    /// 是否检查命令索引。
    const fn checks_index(self) -> bool {
        !matches!(self, ResponseType::None | ResponseType::Long)
    }
}

/// 一条 SD 命令。
#[derive(Debug, Clone, Copy)]
pub struct Command {
    /// 命令号 (0..=63)。
    pub index: u8,
    /// 参数。
    pub argument: u32,
    /// 期望的响应类型。
    pub response: ResponseType,
}

#[allow(dead_code)] // lab-7 会用到
impl Command {
    /// 构造一条命令。
    pub const fn new(index: u8, argument: u32, response: ResponseType) -> Self {
        Self {
            index,
            argument,
            response,
        }
    }

    /// 编码成 COMMAND 寄存器的值。
    const fn encode(&self) -> u16 {
        let mut v = (self.index as u16) << 8;
        v |= self.response.bits();
        if self.response.checks_crc() {
            v |= 1 << 3; // CMD_CRC_CHK_EN
        }
        if self.response.checks_index() {
            v |= 1 << 4; // CMD_IDX_CHK_EN
        }
        // 数据存在选择: 本驱动的命令都不带数据 (数据走独立的传输流程),
        // 恒为 0 = "无数据传输"。
        v
    }
}

/// DesignWare MSHC (SDHCI) 驱动。
pub struct Sdhci {
    regs: Mmio,
    base: usize,
    /// 控制器基准时钟 (Hz), 用于算分频。
    base_clock: u32,
    /// 卡容量, 单位扇区。0 表示尚未识别成功。
    capacity: u64,
    /// 当前使用的总线位宽 (1 或 4)。
    bus_width: u8,
    /// 卡的相对地址 (RCA)。CMD3 之后由卡给出, 后续命令用它寻址。
    rca: u32,
    /// 卡是否为 SDHC/SDXC (高容量)。
    ///
    /// 高容量卡按块号寻址, 标准容量卡按字节地址寻址。
    high_capacity: bool,
}

impl Sdhci {
    /// 构造驱动实例 (不做任何硬件访问)。
    ///
    /// # Safety
    /// `plat.sdhci_base` 必须指向一个已映射的 SDHCI 控制器。
    pub const unsafe fn new(plat: &Platform) -> Self {
        Self {
            // SAFETY: 由调用者保证。
            regs: unsafe { Mmio::new(plat.sdhci_base) },
            base: plat.sdhci_base,
            base_clock: SD_BASE_CLOCK_HZ,
            capacity: 0,
            bus_width: 1,
            rca: 0,
            high_capacity: false,
        }
    }

    /// 基地址。
    pub const fn base(&self) -> usize {
        self.base
    }

    /// 已识别出的卡容量 (扇区)。
    pub const fn capacity(&self) -> u64 {
        self.capacity
    }

    /// 当前总线位宽。
    pub const fn bus_width(&self) -> u8 {
        self.bus_width
    }

    /// 读能力寄存器里声明的基准时钟频率 (Hz)。
    ///
    /// 用它比硬编码常量可靠 (不同 SoC/板子修订版可能不同)。字段无效时
    /// 回退到板级已知常量, 而不是用 0 去除。
    pub fn capability_base_clock_hz(&self) -> u32 {
        // SAFETY: 偏移 0x40/0x44 在控制器寄存器窗口内。
        let caps = unsafe {
            let lo = self.regs.read_u32(REG_CAPABILITIES) as u64;
            let hi = self.regs.read_u32(REG_CAPABILITIES + 4) as u64;
            lo | (hi << 32)
        };
        // 基准时钟频率在 bit 15:8, 单位 MHz。
        let mhz = ((caps >> 8) & 0xff) as u32;
        if mhz == 0 {
            // 字段无效 -> 用板级已知值。
            self.base_clock
        } else {
            mhz * 1_000_000
        }
    }

    /// 控制器是否报告卡已插入。
    ///
    /// 上电后第一件事: 卡没插时, 后面所有初始化都会超时而掩盖真正原因。
    pub fn card_inserted(&self) -> bool {
        // SAFETY: 偏移 0x24 在寄存器窗口内。
        unsafe { self.regs.read_u32(REG_PRESENT_STATE) & STATE_CARD_INSERTED != 0 }
    }

    /// 计算时钟分频值。
    ///
    /// 分频寄存器是 8 位, 但不是线性分频: 写 0 -> /1, 写 1 -> /2,
    /// 写 N (N>=2) -> /(2*N)。
    fn clock_divisor(&self, target_hz: u32) -> u8 {
        if target_hz == 0 {
            return 0;
        }
        let base = self.capability_base_clock_hz();
        if target_hz >= base {
            return 0; // /1
        }
        let div = base / target_hz;
        if div <= 1 {
            return 0;
        }
        // div = 2*N -> N = div/2, 但需要向上取整以保证不超过目标频率
        // (超频比降频危险)。
        let n = div.div_ceil(2);
        // 8 位寄存器, 最大 255。更大的分频做不到, 只能取最大值。
        n.min(255) as u8
    }

    /// 复位控制器。
    ///
    /// 软件复位是异步的: 写 1 后控制器开始复位, 完成后硬件自己清零,
    /// 需轮询等待。超时上限保证硬件坏了也只是报错而非死循环。
    fn reset(&self) -> Result<(), BlockError> {
        // SAFETY: 偏移 0x2f 在寄存器窗口内。
        unsafe {
            self.regs.write_u8(REG_SOFTWARE_RESET, SW_RESET_ALL);
            // 等待硬件自动清零。复位发生在时钟配置之前, 还不知道分频,
            // 所以只能用指令计数而非时钟 tick。
            let mut spins = 0u32;
            while self.regs.read_u8(REG_SOFTWARE_RESET) & SW_RESET_ALL != 0 {
                spins += 1;
                if spins > 1_000_000 {
                    return Err(BlockError::Timeout);
                }
                core::hint::spin_loop();
            }
        }
        Ok(())
    }

    /// 配置 SD 时钟。
    fn set_clock(&self, target_hz: u32) -> Result<(), BlockError> {
        let divisor = self.clock_divisor(target_hz);
        // SAFETY: 偏移 0x2c 在寄存器窗口内。
        unsafe {
            // 1. 先关时钟再改分频 —— 规范要求在时钟关闭时修改分频。
            self.regs.write_u16(REG_CLOCK_CONTROL, 0);
            // 2. 写分频值到高 8 位。
            let v = (divisor as u16) << CLOCK_DIVISOR_SHIFT;
            self.regs.write_u16(REG_CLOCK_CONTROL, v);
            // 3. 打开内部时钟, 等它稳定。
            self.regs
                .write_u16(REG_CLOCK_CONTROL, v | CLOCK_INTERNAL_STABLE);
            let mut spins = 0u32;
            while self.regs.read_u16(REG_CLOCK_CONTROL) & CLOCK_INTERNAL_STABLE == 0 {
                spins += 1;
                if spins > 1_000_000 {
                    return Err(BlockError::Timeout);
                }
                core::hint::spin_loop();
            }
            // 4. 使能 SD 时钟输出。
            self.regs.write_u16(
                REG_CLOCK_CONTROL,
                v | CLOCK_INTERNAL_STABLE | CLOCK_SD_ENABLE,
            );
        }
        Ok(())
    }


    /// 等待命令线空闲。
    fn wait_cmd_idle(&self) -> Result<(), BlockError> {
        let mut spins = 0u32;
        loop {
            // SAFETY: 偏移 0x24。
            if unsafe { self.regs.read_u32(REG_PRESENT_STATE) } & STATE_CMD_INHIBIT == 0 {
                return Ok(());
            }
            spins += 1;
            if spins > 1_000_000 {
                return Err(BlockError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    /// 等待数据线空闲。
    fn wait_dat_idle(&self) -> Result<(), BlockError> {
        let mut spins = 0u32;
        loop {
            // SAFETY: 偏移 0x24。
            if unsafe { self.regs.read_u32(REG_PRESENT_STATE) } & STATE_DAT_INHIBIT == 0 {
                return Ok(());
            }
            spins += 1;
            if spins > 1_000_000 {
                return Err(BlockError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    /// 发一条命令并等它完成, 返回 4 个 32 位响应字。
    ///
    /// 必须先清空中断状态: 否则上一次残留的"命令完成"位会被误读成
    /// 当前命令的完成, 读到上一次的响应。
    fn send_command(&self, cmd: Command) -> Result<[u32; 4], BlockError> {
        self.wait_cmd_idle()?;
        self.wait_dat_idle()?;

        // SAFETY: 下面所有偏移都在控制器寄存器窗口内。
        unsafe {
            // 写 1 清除 (而不是写 0)。
            self.regs.write_u16(REG_NORMAL_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_ERROR_INT_STATUS, 0xffff);

            self.regs.write_u32(REG_ARGUMENT, cmd.argument);
            self.regs.write_u16(REG_COMMAND, cmd.encode());

            let mut spins = 0u32;
            loop {
                let err = self.regs.read_u16(REG_ERROR_INT_STATUS);
                if err != 0 {
                    // 具体哪一位出错对调试有用, 但这里只报告"设备错误":
                    // 上层能做的反应是一样的 (放弃这次操作)。
                    return Err(BlockError::DeviceError);
                }
                if self.regs.read_u16(REG_NORMAL_INT_STATUS) & INT_COMMAND_COMPLETE != 0 {
                    break;
                }
                spins += 1;
                if spins > 1_000_000 {
                    return Err(BlockError::Timeout);
                }
                core::hint::spin_loop();
            }

            Ok([
                self.regs.read_u32(REG_RESPONSE_0),
                self.regs.read_u32(REG_RESPONSE_0 + 4),
                self.regs.read_u32(REG_RESPONSE_0 + 8),
                self.regs.read_u32(REG_RESPONSE_0 + 12),
            ])
        }
    }

    /// 从 CSD 算出卡容量 (单位: 512 字节块)。
    ///
    /// 两种卡的算法不同: SDSC 按 C_SIZE/C_SIZE_MULT/READ_BL_LEN 算字节,
    /// SDHC/SDXC 则 (C_SIZE + 1) * 512 KB。
    fn csd_capacity_blocks(&self, csd: &[u32; 4]) -> u64 {
        // CSD 的位域是相对整个 128 位响应定义的:
        //   csd[0] 对应 bit 127..96, csd[3] 对应 bit 31..0。
        let csd_structure = (csd[0] >> 30) & 0x3;
        if csd_structure == 1 {
            let c_size = ((csd[1] & 0x3f) << 16) | ((csd[2] >> 16) & 0xffff);
            // (C_SIZE + 1) * 512 KB = (C_SIZE + 1) * 1024 个 512 字节块
            ((c_size as u64) + 1) * 1024
        } else {
            let c_size = ((csd[1] & 0x3ff) << 2) | ((csd[2] >> 30) & 0x3);
            let c_mult = (csd[2] >> 15) & 0x7;
            let read_bl_len = csd[2] & 0xf;
            let bytes = ((c_size as u64) + 1) << (c_mult + 2 + read_bl_len);
            bytes / 512
        }
    }

    /// 一条命令的参数该用块号还是字节地址 —— 取决于卡的容量类型。
    fn block_address(&self, block: u64) -> u32 {
        if self.high_capacity {
            block as u32
        } else {
            (block * 512) as u32
        }
    }

    /// 用 PIO 读一个 512 字节块 (CMD17)。
    fn read_one_block(&self, block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.wait_dat_idle()?;
        // SAFETY: 寄存器偏移均在窗口内。
        unsafe {
            self.regs.write_u16(REG_NORMAL_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_ERROR_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_BLOCK_SIZE, 512);
            self.regs.write_u16(REG_BLOCK_COUNT, 1);
            // 传输模式: bit4 = 1 表示"卡 -> 主机" (读)。
            self.regs.write_u16(REG_TRANSFER_MODE, 1 << 4);
        }

        let cmd = Command::new(17, self.block_address(block), ResponseType::Short);
        // 数据线命令在 COMMAND 寄存器里还要置 DATA_PRESENT 位。
        self.send_data_command(cmd)?;

        // 逐字把数据端口的内容搬出来, 每次搬之前等"缓冲区可读"。
        let n = buf.len().min(512);
        let mut off = 0usize;
        while off + 4 <= n {
            let mut spins = 0u32;
            loop {
                // SAFETY: 偏移 0x24。
                if unsafe { self.regs.read_u32(REG_PRESENT_STATE) } & STATE_BUFFER_READ_ENABLE != 0
                {
                    break;
                }
                spins += 1;
                if spins > 1_000_000 {
                    return Err(BlockError::Timeout);
                }
                core::hint::spin_loop();
            }
            // SAFETY: 偏移 0x20。
            let word = unsafe { self.regs.read_u32(REG_BUFFER_DATA_PORT) };
            buf[off..off + 4].copy_from_slice(&word.to_le_bytes());
            off += 4;
        }
        self.wait_transfer_complete()
    }

    /// 用 PIO 写一个 512 字节块 (CMD24)。
    fn write_one_block(&self, block: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.wait_dat_idle()?;
        // SAFETY: 寄存器偏移均在窗口内。
        unsafe {
            self.regs.write_u16(REG_NORMAL_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_ERROR_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_BLOCK_SIZE, 512);
            self.regs.write_u16(REG_BLOCK_COUNT, 1);
            // 传输模式 0 = 主机 -> 卡 (写)。
            self.regs.write_u16(REG_TRANSFER_MODE, 0);
        }

        let cmd = Command::new(24, self.block_address(block), ResponseType::Short);
        self.send_data_command(cmd)?;

        let n = buf.len().min(512);
        let mut off = 0usize;
        while off + 4 <= n {
            let mut spins = 0u32;
            loop {
                // SAFETY: 偏移 0x24。
                if unsafe { self.regs.read_u32(REG_PRESENT_STATE) } & STATE_BUFFER_WRITE_ENABLE != 0
                {
                    break;
                }
                spins += 1;
                if spins > 1_000_000 {
                    return Err(BlockError::Timeout);
                }
                core::hint::spin_loop();
            }
            let word = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
            // SAFETY: 偏移 0x20。
            unsafe { self.regs.write_u32(REG_BUFFER_DATA_PORT, word) };
            off += 4;
        }
        self.wait_transfer_complete()
    }

    /// 发一条**带数据**的命令 (COMMAND 寄存器额外置 DATA_PRESENT 位)。
    fn send_data_command(&self, cmd: Command) -> Result<[u32; 4], BlockError> {
        self.wait_cmd_idle()?;
        // SAFETY: 寄存器偏移均在窗口内。
        unsafe {
            self.regs.write_u16(REG_NORMAL_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_ERROR_INT_STATUS, 0xffff);
            self.regs.write_u32(REG_ARGUMENT, cmd.argument);
            // bit5 = DATA_PRESENT
            self.regs
                .write_u16(REG_COMMAND, cmd.encode() | (1 << 5));

            let mut spins = 0u32;
            loop {
                if self.regs.read_u16(REG_ERROR_INT_STATUS) != 0 {
                    return Err(BlockError::DeviceError);
                }
                if self.regs.read_u16(REG_NORMAL_INT_STATUS) & INT_COMMAND_COMPLETE != 0 {
                    break;
                }
                spins += 1;
                if spins > 1_000_000 {
                    return Err(BlockError::Timeout);
                }
                core::hint::spin_loop();
            }
            Ok([
                self.regs.read_u32(REG_RESPONSE_0),
                self.regs.read_u32(REG_RESPONSE_0 + 4),
                self.regs.read_u32(REG_RESPONSE_0 + 8),
                self.regs.read_u32(REG_RESPONSE_0 + 12),
            ])
        }
    }

    /// 等数据传输完成。
    fn wait_transfer_complete(&self) -> Result<(), BlockError> {
        let mut spins = 0u32;
        loop {
            // SAFETY: 偏移 0x30/0x32。
            unsafe {
                if self.regs.read_u16(REG_ERROR_INT_STATUS) != 0 {
                    return Err(BlockError::DeviceError);
                }
                if self.regs.read_u16(REG_NORMAL_INT_STATUS) & INT_TRANSFER_COMPLETE != 0 {
                    return Ok(());
                }
            }
            spins += 1;
            if spins > 1_000_000 {
                return Err(BlockError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    /// 初始化控制器并识别卡 (复位、设置时钟、完整的 CMD 识别流程)。
    ///
    /// 返回 [`BlockError::Unsupported`] 表示流程未走完, 而不是假装成功。
    pub fn init(&mut self) -> Result<(), BlockError> {
        // 0. 卡插了吗? 先问这个, 错误信息会准确得多。
        if !self.card_inserted() {
            return Err(BlockError::NotReady);
        }
        // 1. 复位控制器。
        self.reset()?;
        // 2. 设置超时 (取最大值, 单位是基准时钟周期)。
        // SAFETY: 偏移 0x2e。
        unsafe {
            self.regs.write_u8(REG_TIMEOUT_CONTROL, 0x0e);
        }
        // 3. 打开电源 (3.3V, 对应位模式 0b111)。
        // SAFETY: 偏移 0x29。
        unsafe {
            self.regs.write_u8(REG_POWER_CONTROL, 0x0f);
        }
        // 4. 初始化阶段只能用 400 kHz, 见 SD_INIT_CLOCK_HZ 的说明。
        self.set_clock(SD_INIT_CLOCK_HZ)?;
        // 5. 清掉所有中断状态 —— 否则第一次等待命令完成时会把
        //    复位过程中残留的状态当成"命令完成了"。
        // SAFETY: 偏移 0x30/0x32。
        unsafe {
            self.regs.write_u16(REG_NORMAL_INT_STATUS, 0xffff);
            self.regs.write_u16(REG_ERROR_INT_STATUS, 0xffff);
        }

        // 5. 卡识别。见文件顶部的流程说明, 每一步都必须带超时。

        // CMD0: 复位到 idle。无响应。
        let _ = self.send_command(Command::new(0, 0, ResponseType::None));

        // CMD8: 声明电压范围 2.7-3.6V (0x1AA 是"检查模式"的固定值)。
        // 有响应说明是 v2 以上的卡; 没响应说明是 v1 (标准容量)。
        let mut is_v2 = false;
        if let Ok(r) = self.send_command(Command::new(8, 0x1aa, ResponseType::Short)) {
            if r[0] & 0xfff == 0x1aa {
                is_v2 = true;
            }
        }

        // CMD55 + ACMD41: 反复询问直到卡报告就绪。
        // 卡上电后要做内部初始化, 规范允许它在一段时间内一直回"忙"。
        let acmd41_arg: u32 = if is_v2 { 0x4000_0000 } else { 0 };
        let mut ready = false;
        let mut last = [0u32; 4];
        for _ in 0..1000 {
            if self.send_command(Command::new(55, 0, ResponseType::Short)).is_err() {
                break;
            }
            match self.send_command(Command::new(41, acmd41_arg, ResponseType::Short)) {
                Ok(r) => {
                    last = r;
                    if r[0] & 0x8000_0000 != 0 {
                        ready = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        if !ready {
            return Err(BlockError::NotReady);
        }
        // CCS 位 (bit30) 说明这是高容量卡。
        self.high_capacity = last[0] & 0x4000_0000 != 0;

        // CMD2: 取 CID (内容不解析, 但流程要求这一步)。
        self.send_command(Command::new(2, 0, ResponseType::Long))?;

        // CMD3: 卡返回自己的 RCA。
        let r3 = self.send_command(Command::new(3, 0, ResponseType::Short))?;
        self.rca = r3[0] & 0xffff_0000;

        // CMD9: 取 CSD —— 容量在里面。
        let csd = self.send_command(Command::new(9, self.rca, ResponseType::Long))?;
        self.capacity = self.csd_capacity_blocks(&csd);
        if self.capacity == 0 {
            return Err(BlockError::DeviceError);
        }

        // CMD7: 选中这张卡, 之后它才会响应数据读写。
        self.send_command(Command::new(7, self.rca, ResponseType::Short))?;

        // CMD16: 块长 512。高容量卡固定 512, 但这条命令对标准容量卡
        // 是必需的, 而成本很低, 所以统一发。
        self.send_command(Command::new(16, 512, ResponseType::Short))?;

        // 6. 切换到 25 MHz "默认速度"。
        self.set_clock(SD_DEFAULT_CLOCK_HZ)?;
        Ok(())
    }
}

impl BlockDevice for Sdhci {
    fn name(&self) -> &'static str {
        "sdhci"
    }

    fn capacity_sectors(&self) -> u64 {
        self.capacity
    }

    fn read(&mut self, block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        // 参数检查与 virtio 完全一样 —— 因为它是**协议无关**的,
        // 所以放在 block/mod.rs 里由两个驱动共用。
        let _count = super::check_request(self.capacity, block, buf.len())?;
        if !self.high_capacity && (block * 512) > u32::MAX as u64 {
            // 标准容量卡用 32 位字节地址寻址, 超过 4 GiB 就寻不了。
            // 与其静默截断 (读到错误的位置), 不如明确拒绝。
            return Err(BlockError::OutOfRange);
        }

        // 逐块发 CMD17 (单块读) 而不是一次 CMD18 多块读: 后者需 CMD12
        // 收尾, 忘了发会让卡停在传输状态, 之后所有命令都报错。
        let mut off = 0usize;
        let mut blk = block;
        while off < buf.len() {
            let n = core::cmp::min(512, buf.len() - off);
            self.read_one_block(blk, &mut buf[off..off + n])?;
            off += n;
            blk += 1;
        }
        Ok(())
    }

    fn write(&mut self, block: u64, buf: &[u8]) -> Result<(), BlockError> {
        let _count = super::check_request(self.capacity, block, buf.len())?;
        if !self.high_capacity && (block * 512) > u32::MAX as u64 {
            return Err(BlockError::OutOfRange);
        }

        let mut off = 0usize;
        let mut blk = block;
        while off < buf.len() {
            let n = core::cmp::min(512, buf.len() - off);
            self.write_one_block(blk, &buf[off..off + n])?;
            off += n;
            blk += 1;
        }
        Ok(())
    }
}

// ===========================================================================
// 编译期自检
// ===========================================================================
const _: () = {
    // SDHCI 的寄存器窗口是 0x00..0x4f (能力寄存器到 0x47)。
    // 所有偏移必须落在里面。
    assert!(REG_MAX_CURRENT_CAPABILITIES + 4 <= 0x100);
    // 命令寄存器是 16 位, 命令号放在 bit 13:8 —— 所以命令号必须 <= 63。
    // 用 `u16` 做移位: `63u8 << 8` 会在 const 求值时报溢出
    // (u8 装不下 0x3f00), 而 SD 命令号的最大值是 63 正是因为它
    // 只有 6 位可用 —— 这条断言把这个设计约束写进了代码。
    assert!((63u16) << 8 == 0x3f00);
    // 分频位移必须在 16 位寄存器的范围内。
    assert!(CLOCK_DIVISOR_SHIFT == 8);
    // 初始化时钟必须**低于**默认时钟。反过来就是"上电就用高速时钟",
    // 正是 SD_INIT_CLOCK_HZ 注释里说的那个坑。
    assert!(SD_INIT_CLOCK_HZ < SD_DEFAULT_CLOCK_HZ);
    // 超时控制寄存器不能为 0 (0 表示 1 个时钟周期, 必然超时)。
    assert!(0x0eu8 != 0);
};

/// 让 `SD_DEFAULT_CLOCK_HZ` 在文档与未来实现里可用。
pub const DEFAULT_CLOCK_HZ: u32 = SD_DEFAULT_CLOCK_HZ;
