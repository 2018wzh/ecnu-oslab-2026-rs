#![no_std]
#![no_main]
use oslab_user::syscall::*;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    // 手动改 path 和 argv 选组，默认交互 test_1。
    let path = c"./test_1";
    let argv: [*const u8; 5] = [c"test_1".as_ptr().cast(), c"111".as_ptr().cast(), c"222".as_ptr().cast(), c"333".as_ptr().cast(), core::ptr::null()];
    // SAFETY: 单线程 init，允许 fork 地址空间复制；未持有内核文件以外的共享所有权。
    let pid = unsafe { fork() };
    if pid < 0 { write(2, b"initcode: fork fail!\n"); }
    else if pid == 0 {
        write(1, b"run "); write(1, path.to_bytes());
        for arg in argv.iter().take(4) {
            write(1, b" ");
            // SAFETY: argv 中这四项来自静态 CStr，指针有效。
            write(1, unsafe { core::ffi::CStr::from_ptr((*arg).cast()) }.to_bytes());
        }
        write(1, b"\n======== test start ========\n\n");
        // SAFETY: argv 最后一项为空，其余四项是存活的静态 CStr。
        unsafe { exec(path, argv.as_ptr()); }
        write(2, b"initcode: exec fail!\n"); exit(1);
    } else {
        let mut status = 0;
        // SAFETY: 独占 i32 输出对象，等待期间存活。
        let waited = unsafe { wait(&mut status) };
        if waited >= 0 && status == 0 { write(1, b"\n======== test success ========\n"); }
        else { write(1, b"\n======== test fail ========\n"); }
    }
    loop { core::hint::spin_loop(); }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { exit(1) }
