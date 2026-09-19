//! RISC-V 64 位实现 (facade 的具体一侧)。只含 CPU/ISA 语义,
//! 不含设备地址与 CPU 数量 (那些在 [`crate::platform`])。


/// 汇编写的 trap 入口 (供 [`trap::install_vector`] 安装)。
pub use trap_entry::TRAP_ENTRY;

/// 汇编写的从核引导入口 (供 [`smp::start_others`] 使用)。
pub use smp_entry::SECONDARY_ENTRY;

// 把 trap 模块里最常用的几个类型提到 arch 顶层, 上层写
// `arch::TrapFrame` 比 `arch::trap::TrapFrame` 更自然。
pub use trap::{TrapCause, N_REGISTERS, TRAPFRAME_SIZE};
pub use trap::TrapFrame;

/// 驱动与设备通信的典型模式是"写内存 -> 通知设备"。这两步之间的
/// 顺序**必须**由屏障保证, 否则设备可能看到未写完的数据。
///
/// 但"用什么指令表达屏障"是**架构相关的**: RISC-V 是 `fence`,
/// AArch64 是 `dmb`。所以驱动只调用 `arch::barrier::io()`,
/// 由 arch 层决定底下是什么。
pub mod barrier {
    /// 全屏障 (读写都排序)。
    #[inline]
    pub fn full() {
        super::csr::fence();
    }
    /// 写-写屏障。
    #[inline]
    pub fn write() {
        super::csr::fence_w();
    }
    /// 设备 I/O 屏障 —— "写内存 -> 通知设备"之间必须用它。
    ///
    /// 见 `csr::fence_io` 的说明: 用错屏障的 bug 表现为
    /// "加一句打印就好了"的时序不稳定。
    #[inline]
    pub fn io() {
        super::csr::fence_io();
    }
    /// 指令缓存同步 (写完代码准备执行时用)。
    #[inline]
    pub fn icache() {
        super::csr::fence_i();
    }
}

/// 本架构在 ELF 文件头 `e_machine` 字段里的编号。
///
/// "哪种机器码这个内核能跑"是架构的事实, 所以放这里而非 ELF 加载器。
pub const ELF_MACHINE: u16 = 243;

/// 本架构在 ELF 文件头 `EI_DATA` 字段里的字节序 (1 = 小端)。
pub const ELF_DATA: u8 = 1;

/// 本架构可用的反汇编工具名 (仅报错提示用)。
///
/// 用 rustup 的 `llvm-objdump` (来自 llvm-tools 组件), 学生只有 rustup
/// 就能用它反汇编核内心像; 不再指 `riscv64-elf-objdump`。
pub const OBJDUMP: &str = "llvm-objdump";

// ---- 各子系统模块 ----
pub mod boot;
pub mod cpu;
// `csr` 是 CSR 访问实现。generic 代码应走导出的语义化接口, 不直接用 CSR 名。
pub mod csr;
pub mod irq;
pub mod sbi;
pub mod time;
pub mod trap;
pub mod smp;
mod smp_entry;
pub mod mm;
mod trap_entry;
// ---- 本阶段模块列表结束 ----

