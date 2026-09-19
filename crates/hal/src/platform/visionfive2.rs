//! visionfive2 — VisionFive2 (昉·星光2), SoC: StarFive JH7110。
//!
//! 与 `qemu_virt.rs` 结构相同、数值不同: 结构相同说明 platform 层提供
//! 统一描述界面, 数值不同说明差异被正确隔离在 platform 层内。主要差异:
//! DRAM 基址 0x40000000、hart 区间 1..4 (不含 0)、定时器全走 SBI、
//! 块设备为 SD 卡 —— 前四项是参数差异, 只有块设备是实现选型差异
//! (经 trait 拿到, 不是 `#[cfg]`)。

use super::{BlockKind, HartRange, MemRegion, Platform};

/// VisionFive2 / JH7110 的静态描述。
pub const VISIONFIVE2: Platform = Platform {
    name: "visionfive2",
    arch: "riscv64",

    // ---- 物理内存 --------------------------------------------------------
    // JH7110 DRAM 从 0x40000000 开始 (与 QEMU 不同, 但 kernel 不改一个数字)。
    dram_base: 0x4000_0000,
    // 板载 2/4/8 GB, 这里保守只用前 128 MiB: 教学够用、U-Boot 可能占用
    // 高端地址、且 128 MiB 是自洽检查点 (真实内存更小应先怀疑常量)。
    dram_size: 128 * 1024 * 1024,
    // OpenSBI 占 0x40000000 起, 跳转到 0x40200000 (与 QEMU 不同)。
    kernel_base: 0x4020_0000,
    firmware_base: 0x4000_0000,

    // ---- CPU 拓扑 --------------------------------------------------------
    // 4+1 核: 四个 U74 应用核 + 一个 S7 监控核 (hart 0, 被 U-Boot 占用)。
    // 内核可用 hart 1..4。起点非 0, `hartid < ncpu` 的判断都错 (详见
    // HartRange 说明); 栈索引须用 `hartid - harts.min`。
    ncpu: 4,
    harts: HartRange::new(1, 4),
    // U-Boot 把内核跑在 hart 1 上。
    boot_hart: 1,

    // ---- 中断控制器 ------------------------------------------------------
    // 同样 SiFive PLIC, 基址与 QEMU 不同; 驱动只依赖 platform 基址即可。
    plic_base: 0x0c00_0000,
    plic_size: 0x0400_0000,

    // ---- 定时器 ----------------------------------------------------------
    // timebase 4 MHz (QEMU 10 MHz); 400_000 tick / 4 MHz = 0.1 秒,
    // 与 QEMU 的"每 0.1 秒调度一次"对齐。用了 QEMU 的 1_000_000 会
    // 让 VF2 切换明显更慢, 是查不到就发现不了的移植 bug。
    timer_interval: 400_000,

    // ---- 地址空间边界 ----------------------------------------------------
    // 设备 MMIO 最低地址 (CLINT 在 0x0200_0000)。
    devices_base: 0x0200_0000,

    // ---- 串口 ------------------------------------------------------------
    // 同样是 16550 兼容 (地址 0x10000000 与 QEMU 相同是巧合, 不代表
    // 地址可以不放 platform 层)。
    uart0_base: 0x1000_0000,
    // 中断号 32 (QEMU 是 10)。
    uart0_irq: 32,
    // 输入时钟 24 MHz (非 QEMU 的 3.6864 MHz)。用错则分频错、输出乱码。
    uart0_clock: 24_000_000,

    // ---- 块设备 ----------------------------------------------------------
    // 无 VirtIO, 用真实 microSD (Synopsys DesignWare MSHC, SDHCI 兼容)。
    block: BlockKind::DesignWareMshc,
    // 无 virtio-mmio 槽位, 填 0 (理由同 qemu_virt.rs)。
    virtio0_base: 0,
    virtio0_irq: 0,
    virtio_count: 0,
    sdhci_base: 0x1600_0000,
    // 本课程用轮询, 不注册 SD 中断 (真机 SD 中断初始化需要设备树, 是
    // 本课程刻意不引入的)。
    sdhci_irq: 0,
};

// ===========================================================================
// 编译期自检 (与 qemu_virt.rs 一一对应)
// ===========================================================================
const _: () = {
    // 1. hart 区间与 ncpu 自洽 (删掉它, VF2"只起来 3 个核"的 bug 就复现)。
    assert!(VISIONFIVE2.harts.count() == VISIONFIVE2.ncpu);

    // 2. 启动 hart 必须在区间内。
    assert!(VISIONFIVE2.harts.contains(VISIONFIVE2.boot_hart));

    // 3. hart 0 是 S7 监控核, 绝对不能被内核使用 (写成断言而非注释,
    //    防"顺手把 min 改成 0"导致真机随机卡死)。
    assert!(VISIONFIVE2.harts.min > 0);
    assert!(!VISIONFIVE2.harts.contains(0));

    // 4. 内存布局自洽。
    assert!(VISIONFIVE2.kernel_base > VISIONFIVE2.firmware_base);
    assert!(VISIONFIVE2.dram().contains(VISIONFIVE2.kernel_base));

    // 5. 对齐要求。
    assert!(VISIONFIVE2.dram_base % 4096 == 0);
    assert!(VISIONFIVE2.kernel_base % 4096 == 0);
    assert!(VISIONFIVE2.uart0_base % 4096 == 0);
    assert!(VISIONFIVE2.plic_base % 4096 == 0);
    assert!(VISIONFIVE2.sdhci_base % 4096 == 0);

    // 6. 块设备类型与地址一致。
    assert!(matches!(VISIONFIVE2.block, BlockKind::DesignWareMshc));
    assert!(VISIONFIVE2.sdhci_base != 0);

    // 7. 该平台不走 CLINT 时间源。
    assert!(VISIONFIVE2.devices_base == 0x0200_0000);
};

/// 地址清单, 供 `docs/porting.md` 的表格与 `xtask list --verbose` 使用。
///
/// 不含 CLINT: 读时间走 `time` CSR、设中断走 SBI, 内核从不访问 CLINT
/// (真机上 CLINT 对 S-mode 的可见性因 SoC 而异, 绕开它正是可移植性来源)。
pub const ADDRESS_MAP: &[(&str, usize, &str)] = &[
    (
        "DRAM",
        VISIONFIVE2.dram_base,
        "内存起点 (JH7110 从 0x40000000 开始)",
    ),
    (
        "kernel",
        VISIONFIVE2.kernel_base,
        "OpenSBI 之后的第一个可用地址",
    ),
    (
        "PLIC",
        VISIONFIVE2.plic_base,
        "中断控制器 (与 QEMU 的 0x0c000000 不同)",
    ),

    (
        "UART0",
        VISIONFIVE2.uart0_base,
        "16550 兼容, 但时钟是 24 MHz",
    ),
    ("SDIO0", VISIONFIVE2.sdhci_base, "DW MSHC SD 控制器"),
];

/// DRAM 区域。
pub const DRAM: MemRegion = VISIONFIVE2.dram();
