#![no_std]
#![no_main]
use oslab_user::{syscall::*, test::{check, print}};
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { exit(1) }
// test_1：参数观察与 stdin/stdout/stderr 交互。
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub unsafe extern "C" fn _start(argc: usize, argv: *const *const core::ffi::c_char) -> ! {
    print(1, format_args!("get {} argument:\n", argc));
    for i in 0..argc {
        // SAFETY: exec 提供 argc 项有效且以 NUL 结束的参数，读取不超过数组。
        let arg = unsafe { core::ffi::CStr::from_ptr(*argv.add(i)) };
        print(1, format_args!("arg {} = ", i)); write(1, arg.to_bytes()); write(1, b"\n");
    }
    write(1, b"INPUT: ");
    let mut input = [0; 128]; let n = read(0, &mut input[..127]);
    check(n <= 127); write(1, b"OUTPUT: "); write(1, &input[..n]); write(2, &input[..n]);
    exit(0)
}
