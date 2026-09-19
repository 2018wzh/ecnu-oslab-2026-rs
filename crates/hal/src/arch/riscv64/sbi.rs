//! OpenSBI 控制台和 hart 启动接口。
#[derive(Debug)]
pub struct SbiError(pub isize);
pub fn call(extension: usize, function: usize, x: usize, y: usize, z: usize) -> isize {
    let result: isize;
    // SAFETY: 按 SBI 调用约定声明输入、输出及被修改的寄存器。
    unsafe {
        core::arch::asm!("ecall", inlateout("a0") x => result,
            inlateout("a1") y => _, in("a2") z, in("a6") function, in("a7") extension);
    }
    result
}
pub fn putc(c: u8) { let _ = call(1, 0, c as usize, 0, 0); }
pub fn hart_start(hart: usize, entry: usize) -> Result<(), SbiError> {
    let error = call(0x48534d, 0, hart, entry, 0);
    if error == 0 { Ok(()) } else { Err(SbiError(error)) }
}
