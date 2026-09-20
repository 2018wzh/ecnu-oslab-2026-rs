#![no_std]
//! 用户与内核共享的调用编号，不含架构指令。
pub const SYS_HELLO: usize = 0;
pub const E_NOSYS: isize = -38;
