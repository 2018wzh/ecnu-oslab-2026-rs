#![no_std]
#![no_main]
use oslab_user::{syscall::*, test::{check, print, dentries}};
use oslab_uapi::*;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { exit(1) }
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    let dir = open(c"dev", OPEN_READ); check(dir >= 0);
    let mut entries = [0; 640]; let n = get_dentries(dir as usize, &mut entries);
    check(n >= 0 && n as usize <= entries.len()); dentries(&entries[..n as usize], "dev"); close(dir as usize);
    check(chdir(c"dev") == 0);
    let fd = open(c"zero", OPEN_READ); check(fd >= 0); let mut tmp = [0xff; 128];
    check(read(fd as usize, &mut tmp) == 128);
    for word in tmp.chunks_exact(4) { print(1, format_args!("{} ", u32::from_le_bytes(word.try_into().unwrap()))); }
    write(1, b"\n"); close(fd as usize);
    let fd = open(c"null", OPEN_READ | OPEN_WRITE); check(fd >= 0);
    check(write(fd as usize, &tmp) == 128); check(read(fd as usize, &mut tmp) == 0); close(fd as usize);
    let fd = open(c"gpt0", OPEN_WRITE); check(fd >= 0);
    for i in 1..=4 {
        print(1, format_args!("Q{}: ", i)); let mut line = [0; 128]; let n = read(0, &mut line[..127]); check(n <= 127);
        let n = if n > 0 && line[n - 1] == b'\n' { n - 1 } else { n };
        print(1, format_args!("A{}: ", i)); write(fd as usize, &line[..n]);
    }
    close(fd as usize); exit(0)
}
