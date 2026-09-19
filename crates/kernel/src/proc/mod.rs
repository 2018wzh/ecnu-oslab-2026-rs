//! 进程与调度。模块拆法 (按职责而非行数划文件):
//!   proc.rs     进程结构体 (状态、进程表、内核栈、trapframe 位置)
//!   context.rs  被换出时保存的寄存器集合 (callee-saved)
//!   switch.rs   换出/换入的汇编实现
//!   user.rs     进入用户态之前要准备什么
//!   elf.rs      ELF64 加载器 (从磁盘装入用户程序)
//! 分成多文件是因为读者不同 ("想知道进程字段"读 proc.rs, "想知道切换
//! 汇编"读 switch.rs), 一个大 proc.c 会让这些问题混在一起。

// ---- 各子系统模块 ----
pub mod context;
pub mod proc;
pub mod switch;
pub mod user;
// ---- 本阶段模块列表结束 ----

// 把最常用的名字提到 proc 顶层, 让上层写 `proc::Proc` 而非
// `proc::proc::Proc`, 且内部重组时调用点不用改。
pub use proc::{
    current, current_pid, proc_alloc, proc_at, proc_copy, proc_init, sched_init_hart,
    sched_yield, set_current, sleep, wait_lock, wakeup, Proc, ProcState, KSTACK_SIZE,
    NPROC,
};