#[cfg(target_arch = "riscv64")]
mod riscv64;
#[cfg(target_arch = "riscv64")]
pub use riscv64::syscall6;
#[cfg(not(target_arch = "riscv64"))]
compile_error!("用户库需要受支持的裸机目标；请使用 cargo xtask build");
