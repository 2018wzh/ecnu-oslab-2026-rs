#![no_std]
#![no_main]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    if oslab_user::hello() != 0 { loop { core::hint::spin_loop(); } }
    if oslab_user::hello() != 0 { loop { core::hint::spin_loop(); } }
    loop { core::hint::spin_loop(); }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
