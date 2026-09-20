#![no_std]
//! lab-6 系统调用编号。
pub const SYS_BRK: usize = 1;
pub const SYS_MMAP: usize = 2;
pub const SYS_MUNMAP: usize = 3;
pub const SYS_PRINT_STR: usize = 4;
pub const SYS_PRINT_INT: usize = 5;
pub const SYS_GETPID: usize = 6;
pub const SYS_FORK: usize = 7;
pub const SYS_WAIT: usize = 8;
pub const SYS_EXIT: usize = 9;
pub const SYS_SLEEP: usize = 10;
pub const E_BADARG: isize = -1;
