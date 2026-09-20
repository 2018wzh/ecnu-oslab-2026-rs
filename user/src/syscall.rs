//! 通用用户调用接口；寄存器约定由 arch 层处理。
/// # Safety
/// 调用者必须满足具体调用的指针、内存修改及生命周期契约。
pub unsafe fn syscall6(number: usize, args: [usize; 6]) -> isize {
    // SAFETY: 将调用者保证的契约传递给所选架构。
    unsafe { crate::arch::syscall6(number, args) }
}
/// # Safety
/// 调整后不保留指向已释放堆页的引用；调用者管理堆对象的存活期。
pub unsafe fn brk(top: usize) -> isize {
    // SAFETY: 继承调用者的堆所有权约定。
    unsafe { syscall6(oslab_uapi::SYS_BRK, [top, 0, 0, 0, 0, 0]) }
}
/// # Safety
/// 调用者管理新区域，检查返回值后才访问；不得与存活对象重叠。
pub unsafe fn mmap(address: usize, len: usize) -> isize {
    // SAFETY: 继承调用者的地址空间约定。
    unsafe { syscall6(oslab_uapi::SYS_MMAP, [address, len, 0, 0, 0, 0]) }
}
/// # Safety
/// 解除前结束区域内所有引用和访问；返回后不得继续使用原地址。
pub unsafe fn munmap(address: usize, len: usize) -> isize {
    // SAFETY: 继承调用者的独占访问约定。
    unsafe { syscall6(oslab_uapi::SYS_MUNMAP, [address, len, 0, 0, 0, 0]) }
}

pub fn getpid() -> isize {
    // SAFETY: 无指针参数。
    unsafe { syscall6(oslab_uapi::SYS_GETPID, [0; 6]) }
}
/// # Safety
/// 调用点必须允许单线程地址空间复制；子进程不继承可跨进程共享的 Rust 所有权。
pub unsafe fn fork() -> isize {
    // SAFETY: 调用者允许分叉执行。
    unsafe { syscall6(oslab_uapi::SYS_FORK, [0; 6]) }
}
/// # Safety
/// status 为零或有效且独占可写的 i32 用户地址。
pub unsafe fn wait(status: *mut i32) -> isize {
    // SAFETY: 调用者保证状态缓冲有效；零表示忽略状态。
    unsafe { syscall6(oslab_uapi::SYS_WAIT, [status as usize, 0, 0, 0, 0, 0]) }
}
pub fn exit(status: i32) -> ! {
    // SAFETY: 终止当前进程，纯值参数。
    unsafe { syscall6(oslab_uapi::SYS_EXIT, [status as usize, 0, 0, 0, 0, 0]); }
    loop { core::hint::spin_loop(); }
}
pub fn sleep(ticks: usize) -> isize {
    // SAFETY: 纯值参数。
    unsafe { syscall6(oslab_uapi::SYS_SLEEP, [ticks, 0, 0, 0, 0, 0]) }
}

use core::ffi::CStr;
use oslab_uapi::*;
/// # Safety
/// argv 指向 NUL 指针终止的列表，各字符串存活且以 NUL 结束；最多 32 项，每项含 NUL 最多 128 字节。
pub unsafe fn exec(path: &CStr, argv: *const *const u8) -> isize {
    // SAFETY: 调用者保证参数列表，path 的有效性由 CStr 保证。
    unsafe { syscall6(SYS_EXEC, [path.as_ptr() as usize, argv as usize, 0, 0, 0, 0]) }
}
pub fn open(path: &CStr, mode: usize) -> isize {
    // SAFETY: path 在同步调用期间有效。
    unsafe { syscall6(SYS_OPEN, [path.as_ptr() as usize, mode, 0, 0, 0, 0]) }
}
pub fn close(fd: usize) -> isize {
    // SAFETY: 纯值参数。
    unsafe { syscall6(SYS_CLOSE, [fd, 0, 0, 0, 0, 0]) }
}
/// ABI 顺序 fd、len、addr；切片保证输出缓冲独占。失败返回 0。
pub fn read(fd: usize, dst: &mut [u8]) -> usize {
    // SAFETY: dst 在调用期间可写且独占。
    unsafe { syscall6(SYS_READ, [fd, dst.len(), dst.as_mut_ptr() as usize, 0, 0, 0]) as usize }
}
/// ABI 顺序 fd、len、addr；失败返回 0。
pub fn write(fd: usize, src: &[u8]) -> usize {
    // SAFETY: src 在调用期间有效且只读。
    unsafe { syscall6(SYS_WRITE, [fd, src.len(), src.as_ptr() as usize, 0, 0, 0]) as usize }
}
pub fn lseek(fd: usize, offset: u32, flag: usize) -> isize {
    // SAFETY: 纯值参数，尽力而为；成功返回新偏移。
    unsafe { syscall6(SYS_LSEEK, [fd, offset as usize, flag, 0, 0, 0]) }
}
pub fn dup(fd: usize) -> isize {
    // SAFETY: 纯值参数。
    unsafe { syscall6(SYS_DUP, [fd, 0, 0, 0, 0, 0]) }
}
pub fn fstat(fd: usize, out: &mut FileStat) -> isize {
    // SAFETY: repr(C) 的 16 字节输出对象在调用期间独占。
    unsafe { syscall6(SYS_FSTAT, [fd, out as *mut _ as usize, 0, 0, 0, 0]) }
}
/// 容量与返回均为字节，失败 -1。
pub fn get_dentries(fd: usize, dst: &mut [u8]) -> isize {
    // SAFETY: 输出切片独占，内核按字节复制目录项。
    unsafe { syscall6(SYS_GET_DENTRIES, [fd, dst.as_mut_ptr() as usize, dst.len(), 0, 0, 0]) }
}
pub fn mkdir(path: &CStr) -> isize {
    // SAFETY: CStr 保证调用期间有效且以 NUL 结束。
    unsafe { syscall6(SYS_MKDIR, [path.as_ptr() as usize, 0, 0, 0, 0, 0]) }
}
pub fn chdir(path: &CStr) -> isize {
    // SAFETY: CStr 保证调用期间有效且以 NUL 结束。
    unsafe { syscall6(SYS_CHDIR, [path.as_ptr() as usize, 0, 0, 0, 0, 0]) }
}
pub fn unlink(path: &CStr) -> isize {
    // SAFETY: CStr 保证调用期间有效且以 NUL 结束。
    unsafe { syscall6(SYS_UNLINK, [path.as_ptr() as usize, 0, 0, 0, 0, 0]) }
}
pub fn print_cwd() -> isize {
    // SAFETY: 无参数，内核负责打印，成功 0、失败 -1。
    unsafe { syscall6(SYS_PRINT_CWD, [0; 6]) }
}
pub fn link(old: &CStr, new: &CStr) -> isize {
    // SAFETY: 两个 CStr 在调用期间稳定存活。
    unsafe { syscall6(SYS_LINK, [old.as_ptr() as usize, new.as_ptr() as usize, 0, 0, 0, 0]) }
}
