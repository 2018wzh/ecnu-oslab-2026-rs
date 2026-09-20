#![no_std]
#![no_main]
use oslab_user::syscall as sys;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    // SAFETY: 受控单进程例程，令牌原样回传，不重复归还，不调用 fork/exit。
    unsafe {
        sys::print_str(c"hello, world!\n".as_ptr().cast());
    }
    loop { core::hint::spin_loop(); }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
