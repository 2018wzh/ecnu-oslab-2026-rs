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

pub mod disk;
pub const SYS_ALLOC_BLOCK: usize = 11;
pub const SYS_FREE_BLOCK: usize = 12;
pub const SYS_ALLOC_INODE: usize = 13;
pub const SYS_FREE_INODE: usize = 14;
pub const SYS_SHOW_BITMAP: usize = 15;
pub const SYS_GET_BLOCK: usize = 16;
pub const SYS_READ_BLOCK: usize = 17;
pub const SYS_WRITE_BLOCK: usize = 18;
pub const SYS_PUT_BLOCK: usize = 19;
pub const SYS_SHOW_BUFFER: usize = 20;
pub const SYS_FLUSH_BUFFER: usize = 21;
