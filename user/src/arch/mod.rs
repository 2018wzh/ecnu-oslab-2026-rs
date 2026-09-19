//! `arch` — 用户态运行时的架构层 (系统调用机制), 通过
//! `#[cfg(feature = "arch-<name>")]` 分发到具体架构。
//!
//! 与内核侧 `hal::arch` 同一套设计: "系统调用怎么做" (RISC-V 用
//! `ecall` + a0-a2/a7 寄存器约定) 是**架构事实**, 必须与架构绑定;
//! 而 `Syscall` 调用号、`write`/`getpid` 等上层封装 (见 `super::runtime`)
//! 与架构无关, 它们只调用这里暴露的 [`syscall_raw`]。
//!
//! 新增一个架构 = 新建 `arch/<arch>.rs` + 加一个 `arch-<arch>` feature +
//! 在这里加一个 `#[cfg(feature = "arch-<arch>")]` 分支, 框架代码其余
//! 不动 (与 hal 的加法人一致, 见 crates/hal/src/arch/mod.rs)。

#[cfg(feature = "arch-riscv64")]
mod riscv64;

#[cfg(feature = "arch-riscv64")]
pub use riscv64::*;

// 必须恰好选中一个架构, 否则报错要直接指出该做什么。
#[cfg(not(feature = "arch-riscv64"))]
compile_error!(
    "没有选择架构。请在构建时显式指定, 例如:\n\
     cargo build -p oslab-user --features arch-riscv64\n\
     (由 xtask 的用户程序构建自动传入, 见 xtask/src/user.rs)\n\
     \n\
     要新增一个架构, 见本文件顶部的说明。"
);