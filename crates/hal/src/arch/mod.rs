//! `hal::arch` — CPU / ISA 语义, 通过 `#[cfg(feature = "arch-<name>")]`
//! 分发到具体架构 (cpu / irq / trap / mm / sbi / time / boot)。
//! 不含设备地址与内存起点 —— 那些属于 [`crate::platform`]。

#[cfg(feature = "arch-riscv64")]
mod riscv64;

#[cfg(feature = "arch-riscv64")]
pub use riscv64::*;

// 必须恰好选中一个架构, 否则报错要直接指出该做什么。
#[cfg(not(feature = "arch-riscv64"))]
compile_error!(
    "没有选择架构。请在 configs/*.toml 里设置 arch, 或用 cargo 显式指定:\n\
     cargo build -p oslab-kernel --features arch-riscv64,platform-qemu-virt\n\
     \n\
     要新增一个架构, 见本文件顶部"加入第二个架构的步骤"。"
);
