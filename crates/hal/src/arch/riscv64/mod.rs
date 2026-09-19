//! RISC-V 64 位实现 (facade 的具体一侧)。只含 CPU/ISA 语义,
//! 不含设备地址与 CPU 数量 (那些在 [`crate::platform`])。



/// 汇编写的从核引导入口 (供 [`smp::start_others`] 使用)。
pub use smp_entry::SECONDARY_ENTRY;

// 把 trap 模块里最常用的几个类型提到 arch 顶层, 上层写
// `arch::TrapFrame` 比 `arch::trap::TrapFrame` 更自然。
pub use trap::TrapFrame;

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
// ---- 本阶段模块列表结束 ----

