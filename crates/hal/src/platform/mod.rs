//! `hal::platform` — 机器语义 (编译期静态常量, 不用设备树)。
//!
//! 一台机器什么样在编译期已完全确定 (DRAM 位置/大小、UART/PLIC/SDIO
//! 地址、中断号、可用 hart)。所以用 `const` 而非运行期 DTB: 教学只需
//! 支持两台机器, 地址都是已知常量; DTB 解决的是"一个二进制跑很多板子"
//! 的问题, 本课程每个配置编译一份内核。代价是加新板子要重编并手抄地址
//! (见 `docs/porting.md`), 但常量会受编译期断言约束。

#[cfg(feature = "platform-qemu-virt")]
mod qemu_virt;
#[cfg(feature = "platform-qemu-virt")]
pub use qemu_virt::*;

#[cfg(feature = "platform-visionfive2")]
mod visionfive2;
#[cfg(feature = "platform-visionfive2")]
pub use visionfive2::*;

/// 当前平台的描述, 是 `const` 而非 `static` (字段被编译期内联成立即数,
/// 上层写 `platform::PLATFORM`, 换平台时不用 `#[cfg]`)。
#[cfg(feature = "platform-qemu-virt")]
pub const PLATFORM: Platform = QEMU_VIRT;

/// 当前平台的描述 (见 [`PLATFORM`])。
#[cfg(feature = "platform-visionfive2")]
pub const PLATFORM: Platform = VISIONFIVE2;

/// 内存区域: 描述"哪一段物理地址是普通 DRAM"。
///
/// `const fn`, 只用来构造 [`Platform`] 常量实例, 不占运行期内存。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemRegion {
    /// 起始物理地址。
    pub base: usize,
    /// 字节长度。
    pub size: usize,
}

impl MemRegion {
    /// 构造。
    pub const fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }

    /// 结束地址 (开区间)。
    pub const fn end(&self) -> usize {
        self.base + self.size
    }

    /// `addr` 是否落在这个区间内。
    pub const fn contains(&self, addr: usize) -> bool {
        addr >= self.base && addr < self.end()
    }
}

/// 一个 hart 的可编号区间, **闭区间**。
///
/// 用闭区间而非 `0..ncpu`: 两个平台起点不同 (QEMU 0..1 / VF2 1..4),
/// `hartid < ncpu` 会漏掉合法 hart 或尝试启动 U-Boot 占用的监控核。
/// 配套编译期断言强制 `max - min + 1 == ncpu`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HartRange {
    /// 第一个可供内核使用的 hart id (闭)。
    pub min: usize,
    /// 最后一个可供内核使用的 hart id (闭)。
    pub max: usize,
}

impl HartRange {
    /// 构造。
    pub const fn new(min: usize, max: usize) -> Self {
        Self { min, max }
    }

    /// hart id 是否合法。**所有**校验都走这个函数, 不自己写比较表达式。
    pub const fn contains(&self, hartid: usize) -> bool {
        hartid >= self.min && hartid <= self.max
    }

    /// hart id -> 内核内部连续 CPU 编号 (0 起)。
    ///
    /// 内核用 `cpuid` 索引 per-cpu 数组 (栈、idle、PLIC 位图), 必须是
    /// 0..ncpu-1; QEMU 上 `hartid == cpuid`, VF2 上差 1。返回 `None`
    /// 表示非法, 让调用者必须处理而非悄悄截断。
    pub const fn to_cpu_id(&self, hartid: usize) -> Option<usize> {
        if self.contains(hartid) {
            Some(hartid - self.min)
        } else {
            None
        }
    }

    /// 内核 CPU 编号 -> hart id。
    pub const fn to_hartid(&self, cpuid: usize) -> Option<usize> {
        let h = self.min + cpuid;
        if self.contains(h) {
            Some(h)
        } else {
            None
        }
    }

    /// 区间内 hart 的个数。
    pub const fn count(&self) -> usize {
        self.max - self.min + 1
    }
}

/// 一台机器的完整静态描述。
///
/// 用一份 struct 的 `const` 实例而非一堆自由 `const`, 让"两个平台提供
/// 完全相同的字段"成为类型系统强制的约束 (缺/多/类型不一致都编译期失败)。
/// 常量实例运行期不占内存, 字段被内联成立即数。
#[derive(Debug, Clone, Copy)]
pub struct Platform {
    /// 人类可读的平台名, 只用于启动横幅与日志。
    pub name: &'static str,
    /// 这份描述对应的架构名。用于启动时自检"构建系统与代码是否一致"。
    pub arch: &'static str,

    // ---- 物理内存 --------------------------------------------------------
    /// DRAM 的起始物理地址。
    pub dram_base: usize,
    /// DRAM 的容量 (字节)。必须与 QEMU 的 `-m` / 板子的实际内存一致。
    pub dram_size: usize,
    /// 内核期望自己被加载到的物理地址, **必须等于链接地址** (来自
    /// `configs/*.toml` 的 `kernel_load_addr`, 由 build.rs 注入链接脚本)。
    /// 不一致时所有绝对地址访问都偏掉; 启动会拿 `_entry` 真实地址来比较。
    pub kernel_base: usize,
    /// 固件 (OpenSBI) 占据的起始地址。这段区间不能被物理页分配器使用。
    pub firmware_base: usize,

    // ---- CPU 拓扑 --------------------------------------------------------
    /// 参与运行内核的 hart 数量。
    pub ncpu: usize,
    /// 内核可用的 hart id 闭区间。见 [`HartRange`] 的说明。
    pub harts: HartRange,
    /// 启动核 (固件把控制权交给它的那个 hart) 的 hart id。
    pub boot_hart: usize,

    // ---- 中断控制器 ------------------------------------------------------
    /// PLIC 基地址。
    pub plic_base: usize,
    /// PLIC 寄存器窗口大小 (用于页表映射时算出要映射多少页)。
    pub plic_size: usize,

    // ---- 定时器 ----------------------------------------------------------
    /// 定时器中断间隔, 单位是 `time` CSR 的 tick 数。
    ///
    /// 这里没有 CLINT 地址: 内核读时间走 `time` CSR (S-mode 可直接读),
    /// 设中断走 SBI (mtimecmp 是 M-mode 寄存器, 有地址也写不了)。
    /// 放 CLINT 地址只会诱导人去写不可移植的 MMIO 读。
    pub timer_interval: usize,

    // ---- 地址空间边界 ----------------------------------------------------
    /// 设备 MMIO 区的**最低**地址。
    ///
    /// 内核用它界定用户地址空间上界 (用户代码/栈/堆须整体位于其下),
    /// 否则页会映射到设备上把它覆盖成普通内存页。这是平台事实, 不能
    /// 写死"够大"的常数 (换板子可能与其设备重叠)。
    pub devices_base: usize,

    // ---- 串口 ------------------------------------------------------------
    /// UART0 基地址 (16550 兼容)。
    pub uart0_base: usize,
    /// UART0 的中断号 (PLIC 编号)。
    pub uart0_irq: u32,
    /// UART 的输入时钟频率 (Hz), 用于算波特率分频。
    ///
    /// 抄错是最经典的移植 bug: 输出全乱码, 且易误判成代码逻辑问题。
    /// 两平台值不同 (3686400 vs 24000000), 但驱动源码一行不改。
    pub uart0_clock: u32,

    // ---- 块设备 ----------------------------------------------------------
    /// 该平台使用的块设备类型。
    pub block: BlockKind,
    /// VirtIO-MMIO 第一个槽位的基地址 (仅 [`BlockKind::VirtioMmio`] 有意义)。
    pub virtio0_base: usize,
    /// VirtIO-MMIO 槽位的中断号。
    pub virtio0_irq: u32,
    /// VirtIO-MMIO 的槽位总数 (QEMU virt 上扫描用)。
    pub virtio_count: usize,
    /// SD 控制器基地址 (仅 [`BlockKind::DesignWareMshc`] 有意义)。
    pub sdhci_base: usize,
    /// SD 控制器中断号。0 表示本课程用轮询, 不注册中断。
    pub sdhci_irq: u32,
}

/// 一台机器使用的块设备类型。
///
/// 用枚举而非 bool: 将来加 NVMe 只需加变体, 所有 `match` 都会被编译器
/// 指出要求处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// QEMU 的半虚拟化块设备, 通过 MMIO 传输层访问。
    VirtioMmio,
    /// 真实的 microSD 卡, 控制器是 Synopsys DesignWare MSHC (SDHCI 兼容)。
    DesignWareMshc,
}

impl Platform {
    /// DRAM 区域。
    pub const fn dram(&self) -> MemRegion {
        MemRegion::new(self.dram_base, self.dram_size)
    }

    /// 固件 (OpenSBI) 区域。物理页分配器必须跳过它。
    pub const fn firmware(&self) -> MemRegion {
        MemRegion::new(self.firmware_base, self.kernel_base - self.firmware_base)
    }
}
