#![no_std]
#![no_main]
use oslab_user::{syscall::*, test::{check, stat, dentries}};
use oslab_uapi::*;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { exit(1) }
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    print_cwd(); mkdir(c"new_workdir"); chdir(c"../.././new_workdir"); print_cwd();
    mkdir(c"./2025_12_22"); mkdir(c"2025_12_22/19:00"); chdir(c"./2025_12_22/19:00"); print_cwd();
    let fd1 = open(c"./hello.txt", OPEN_READ | OPEN_WRITE | OPEN_CREATE); check(fd1 >= 0);
    check(link(c"./hello.txt", c"/link.txt") == 0);
    let fd2 = open(c"../../../link.txt", OPEN_READ | OPEN_WRITE); check(fd2 >= 0);
    write(fd2 as usize, b"hello world!");
    let mut tmp = [0; 32]; let n = read(fd1 as usize, &mut tmp); check(n <= tmp.len());
    write(1, b"read data = "); write(1, &tmp[..n]); write(1, b"\n");
    let mut st = FileStat::default(); check(fstat(fd1 as usize, &mut st) == 0); stat(&st, "hello.txt");
    close(fd1 as usize); close(fd2 as usize); chdir(c"../../.."); print_cwd();
    let root = open(c"/", OPEN_READ); check(root >= 0); let mut entries = [0; 640];
    let n = get_dentries(root as usize, &mut entries); check(n >= 0 && n as usize <= entries.len()); dentries(&entries[..n as usize], "root");
    unlink(c"./link.txt"); unlink(c"./new_workdir/2025_12_22/19:00/hello.txt");
    unlink(c"./new_workdir/2025_12_22/19:00"); unlink(c"./new_workdir/2025_12_22"); unlink(c"./new_workdir");
    let n = get_dentries(root as usize, &mut entries); check(n >= 0 && n as usize <= entries.len()); dentries(&entries[..n as usize], "root");
    close(root as usize); exit(0)
}
