#![no_std]
#![no_main]
use oslab_user::{syscall::*, test::{check, print, stat, dentries}};
use oslab_uapi::*;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { exit(1) }
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    let root = open(c"/", OPEN_READ); check(root >= 0);
    let copy = dup(root as usize); check(copy >= 0);
    let mut st = FileStat::default(); check(fstat(copy as usize, &mut st) == 0); stat(&st, "root"); close(copy as usize);
    // SAFETY: 新匿名区域由本例程独占，先检查返回值；解除前结束切片借用。
    let addr = unsafe { mmap(0, 4096) }; check(addr > 0);
    {
        // SAFETY: 已获得一页映射，无其他别名。
        let tmp = unsafe { core::slice::from_raw_parts_mut(addr as *mut u8, 4096) };
        let fd = open(c"/ABC.txt", OPEN_READ | OPEN_WRITE | OPEN_CREATE); check(fd >= 0); let fd = fd as usize;
        for i in 0..500 { print(fd, format_args!("{}:ABCDEFGHIJKLMNOPQRST ", i)); }
        check(fstat(fd, &mut st) == 0); stat(&st, "ABC.txt");
        check(lseek(fd, 50, LSEEK_SUB) >= 0); check(read(fd, &mut tmp[..50]) == 50);
        write(1, b"read data = "); write(1, &tmp[..50]); write(1, b"\n");
        let n = get_dentries(root as usize, tmp); check(n >= 0 && n as usize <= tmp.len()); dentries(&tmp[..n as usize], "root");
        close(fd); close(root as usize);
    }
    // SAFETY: 区域内的临时切片已结束，不再使用地址。
    unsafe { munmap(addr as usize, 4096); }
    exit(0)
}
