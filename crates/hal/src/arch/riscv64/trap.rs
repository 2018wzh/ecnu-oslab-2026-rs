//! trap 汇编布局及 CSR 接口。
core::arch::global_asm!(include_str!("trap_entry.S"));
#[repr(C)]
pub struct TrapFrame {
    /// 偏移 n*8：x0 为零，x2 记录陷入前 sp；本章不保存浮点/向量状态。
    pub x: [usize; 32],
    /// 偏移 256：返回位置 sepc；不能统一推进 PC。
    pub epc: usize,
    /// 偏移 264：陷入后的 sstatus，包含 SPP/SPIE。
    pub status: usize,
}
const _: () = assert!(core::mem::size_of::<TrapFrame>() == 272);
pub fn cause() -> usize {
    let x;
    // SAFETY: 读取当前核陷阱原因。
    unsafe { core::arch::asm!("csrr {}, scause", out(reg) x); } x
}
pub fn value() -> usize {
    let x;
    // SAFETY: 读取当前核陷阱附加信息。
    unsafe { core::arch::asm!("csrr {}, stval", out(reg) x); } x
}
pub fn install_kernel_vector() {
    unsafe extern "C" { fn kernel_vector(); }
    // SAFETY: 汇编入口对齐且保存完整整数寄存器上下文。
    unsafe { core::arch::asm!("csrw stvec, {}", in(reg) kernel_vector as *const () as usize); }
}
pub fn enable_sources() {
    // SAFETY: 调用者先安装入口、初始化时钟和 PLIC。
    unsafe { core::arch::asm!("csrs sie, {}", in(reg) (1usize << 5) | (1 << 9)); }
}
pub fn time() -> usize {
    let x;
    // SAFETY: 固件允许 S-mode 读取 time。
    unsafe { core::arch::asm!("rdtime {}", out(reg) x); } x
}
/// 设置当前 hart 的绝对 time 截止值；调用者检查 SBI 错误。
pub fn set_timer(deadline: usize) -> Result<(), super::sbi::SbiError> {
    let error = super::sbi::call(0x54494d45, 0, deadline, 0, 0);
    if error == 0 { Ok(()) } else { Err(super::sbi::SbiError(error)) }
}
