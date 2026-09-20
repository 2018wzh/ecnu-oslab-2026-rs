#![no_std]
//! lab-5 临时 ABI；后续章节移除三个 copy 服务并迁移编号。
pub const SYS_HELLO: usize = 0;
pub const SYS_TEST_COPYIN: usize = 1;
pub const SYS_TEST_COPYOUT: usize = 2;
pub const SYS_TEST_COPYINSTR: usize = 3;
pub const SYS_BRK: usize = 4;
pub const SYS_MMAP: usize = 5;
pub const SYS_MUNMAP: usize = 6;
pub const E_BADARG: isize = -1;
