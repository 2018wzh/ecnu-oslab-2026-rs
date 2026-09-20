#[cfg(target_arch = "riscv64")]
mod riscv64;
#[cfg(target_arch = "riscv64")]
pub use riscv64::*;

/// 从架构寄存器布局解码的通用系统调用请求。
pub struct Syscall { pub number: usize, pub args: [usize; 6] }
