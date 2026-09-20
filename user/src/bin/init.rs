#![no_std]
#![no_main]
use oslab_user::syscall as sys;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    if sys::getpid() == 1 {
        // SAFETY: 静态 NUL 字符串在调用期间有效。
        unsafe {
            sys::print_str(c"\nproczero: hello ".as_ptr().cast());
            sys::print_str(c"world!\n".as_ptr().cast());
        }
    }
    loop { core::hint::spin_loop(); }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
