#![no_std]
pub mod disk;
pub const SYS_BRK: usize = 1;
pub const SYS_MMAP: usize = 2;
pub const SYS_MUNMAP: usize = 3;
pub const SYS_FORK: usize = 4;
pub const SYS_WAIT: usize = 5;
pub const SYS_EXIT: usize = 6;
pub const SYS_SLEEP: usize = 7;
pub const SYS_GETPID: usize = 8;
pub const SYS_EXEC: usize = 9;
pub const SYS_OPEN: usize = 10;
pub const SYS_CLOSE: usize = 11;
pub const SYS_READ: usize = 12;
pub const SYS_WRITE: usize = 13;
pub const SYS_LSEEK: usize = 14;
pub const SYS_DUP: usize = 15;
pub const SYS_FSTAT: usize = 16;
pub const SYS_GET_DENTRIES: usize = 17;
pub const SYS_MKDIR: usize = 18;
pub const SYS_CHDIR: usize = 19;
pub const SYS_PRINT_CWD: usize = 20;
pub const SYS_LINK: usize = 21;
pub const SYS_UNLINK: usize = 22;
pub const STR_MAXLEN: usize = 127;
pub const PATH_BYTES: usize = 128;
pub const MAX_ARGS: usize = 32;
pub const ARG_BYTES: usize = 128;
pub const OPEN_CREATE: usize = 1;
pub const OPEN_READ: usize = 2;
pub const OPEN_WRITE: usize = 4;
pub const LSEEK_SET: usize = 0;
pub const LSEEK_ADD: usize = 1;
pub const LSEEK_SUB: usize = 2;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileStat { pub kind: u16, pub nlink: u16, pub size: u32, pub inode_num: u32, pub offset: u32 }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent { pub name: [u8; 60], pub inode_num: u32 }
const _: () = assert!(core::mem::size_of::<FileStat>() == 16);
const _: () = assert!(core::mem::offset_of!(FileStat, offset) == 12);
const _: () = assert!(core::mem::size_of::<Dirent>() == 64);
