//! qemu-virt — QEMU `virt` 机器的静态描述。
//!
//! 数值从 QEMU 源码/命令行抄下 (非猜测), 每个都注明出处, 供 `docs/porting.md`
//! 示范如何把手册数字变成代码常量。可用 `qemu-system-riscv64 -machine
//! virt,dumpdtb=...` 打印 DTB 核对 (仅核对, 运行期不解析 DTB)。

use super::{BlockKind, HartRange, MemRegion, Platform};

/// QEMU `virt` 机器的静态描述。
///
/// `const` 而非 `static`: 字段被编译期内联成立即数, 运行期不占内存
/// (`PLATFORM.uart0_base` 最终变成 `li a0, 0x10000000`)。
pub const QEMU_VIRT: Platform = Platform {
    name: "qemu-virt",
    arch: "riscv64",

    // ---- 物理内存 --------------------------------------------------------
    // DRAM 从 0x80000000 开始 —— QEMU 自己的选择 (真实 JH7110 是 0x40000000),
    // 而非 RISC-V 规范规定, 所以属平台层。
    dram_base: 0x8000_0000,
    // 须与 configs/*.toml 的 qemu.memory = "128M" 一致。比实际内存大时,
    // 物理页分配器会发出指向不存在内存的页框 (写入被静默丢弃, 难调试)。
    dram_size: 128 * 1024 * 1024,
    // OpenSBI 占 [0x80000000, 0x80200000), 跳转到 0x80200000。须与
    // kernel_load_addr 及链接脚本注入的 KERNEL_BASE 一致, 否则静默跑飞。
    kernel_base: 0x8020_0000,
    // 固件起点 = DRAM 起点 (OpenSBI 被加载到 DRAM 最前)。
    firmware_base: 0x8000_0000,

    // ---- CPU 拓扑 --------------------------------------------------------
    // 须与 -smp 一致。大了会去启动不存在的 hart (SBI 返回错误, 若忽略
    // 返回值为静默失败)。
    ncpu: 2,
    // QEMU 上所有 hart 都可用, 编号从 0 开始。
    harts: HartRange::new(0, 1),
    // QEMU 从 hart 0 启动。
    boot_hart: 0,

    // ---- 中断控制器 ------------------------------------------------------
    // SiFive PLIC 内存映射: 0x0c000000 PLIC (4 MiB 窗口),
    // 0x10000000 UART0, 0x10001000 VirtIO MMIO (8 槽位)。
    plic_base: 0x0c00_0000,
    plic_size: 0x0400_0000,

    // ---- 定时器 ----------------------------------------------------------
    // QEMU timebase 10 MHz, 1_000_000 tick = 0.1 秒; 时间怎么读、中断怎么
    // 设都与平台无关 (只需"间隔多少 tick"一个值)。
    timer_interval: 1_000_000,

    // ---- 地址空间边界 ----------------------------------------------------
    // 最低设备是 CLINT (0x0200_0000), 用户地址空间必须整体位于其下。
    devices_base: 0x0200_0000,

    // ---- 串口 ------------------------------------------------------------
    uart0_base: 0x1000_0000,
    uart0_irq: 10,
    // 3.6864 MHz = 115200 * 32 (使分频系数整除, QEMU 选它)。抄错只有乱码。
    uart0_clock: 3_686_400,

    // ---- 块设备 ----------------------------------------------------------
    block: BlockKind::VirtioMmio,
    virtio0_base: 0x1000_1000,
    virtio0_irq: 1,
    // QEMU virt 有 8 个 virtio-mmio 槽位 (各 0x1000 字节); 槽 0 被 virtio0_base 占用。
    virtio_count: 8,
    // 无 SD 控制器, 填 0 而非省略 (struct 强制两平台提供相同字段)。
    sdhci_base: 0,
    sdhci_irq: 0,
};

// ===========================================================================
// 编译期自检
// ===========================================================================
// 每条断言对应一个真实发生过的 bug 类别。

const _: () = {
    // 1. hart 区间与 ncpu 自洽。
    assert!(QEMU_VIRT.harts.count() == QEMU_VIRT.ncpu);

    // 2. 启动 hart 必须落在合法区间内。
    assert!(QEMU_VIRT.harts.contains(QEMU_VIRT.boot_hart));

    // 3. 内核必须在 DRAM 里, 不覆盖固件。
    assert!(QEMU_VIRT.kernel_base >= QEMU_VIRT.firmware_base);
    assert!(QEMU_VIRT.dram().contains(QEMU_VIRT.kernel_base));

    // 4. kernel_base 必须严格大于 firmware_base (等于意味着起点重合)。
    assert!(QEMU_VIRT.kernel_base > QEMU_VIRT.firmware_base);

    // 5. 页表映射要求地址 4 KiB 对齐 (非对齐会让 satp 的 PPN 丢位)。
    assert!(QEMU_VIRT.dram_base % 4096 == 0);
    assert!(QEMU_VIRT.kernel_base % 4096 == 0);
    assert!(QEMU_VIRT.uart0_base % 4096 == 0);
    assert!(QEMU_VIRT.plic_base % 4096 == 0);
    assert!(QEMU_VIRT.virtio0_base % 4096 == 0);

    // 6. 块设备类型与地址一致 (防复制粘贴别平台文件忘了改 block)。
    assert!(matches!(QEMU_VIRT.block, BlockKind::VirtioMmio));
    assert!(QEMU_VIRT.virtio0_base != 0);
};

/// 供 `docs/porting.md` 引用的地址清单, 由 `cargo xtask list --verbose` 打印。
///
/// 不含 CLINT: 内核读时间走 `time` CSR、设中断走 SBI, 从不访问 CLINT。
pub const ADDRESS_MAP: &[(&str, usize, &str)] = &[
    (
        "DRAM",
        QEMU_VIRT.dram_base,
        "内存起点 (无固件时为 0x80000000)",
    ),
    (
        "kernel",
        QEMU_VIRT.kernel_base,
        "OpenSBI 之后的第一个可用地址",
    ),
    ("PLIC", QEMU_VIRT.plic_base, "中断控制器"),
    ("UART0", QEMU_VIRT.uart0_base, "16550 串口"),
    ("VirtIO0", QEMU_VIRT.virtio0_base, "第一个 virtio-mmio 槽位"),
];

/// 供自检使用的 DRAM 区域常量形式 (避免在 kernel 里构造)。
pub const DRAM: MemRegion = QEMU_VIRT.dram();
