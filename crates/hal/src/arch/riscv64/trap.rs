//! trap 汇编布局及 CSR 接口。
core::arch::global_asm!(include_str!("trap_entry.S"));
core::arch::global_asm!(include_str!("trampoline.S"));
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
#[repr(C)]
pub struct UserFrame {
    pub regs: TrapFrame,
    pub kernel_satp: usize, pub kernel_sp: usize, pub kernel_entry: usize, pub kernel_hart: usize,
}
const _: () = {
    assert!(core::mem::offset_of!(UserFrame, kernel_satp) == 272);
    assert!(core::mem::offset_of!(UserFrame, kernel_sp) == 280);
    assert!(core::mem::offset_of!(UserFrame, kernel_entry) == 288);
    assert!(core::mem::offset_of!(UserFrame, kernel_hart) == 296);
    assert!(core::mem::size_of::<UserFrame>() == 304);
};
pub const TRAMPOLINE: usize = super::mm::VA_MAX - 4096;
pub const TRAPFRAME: usize = TRAMPOLINE - 4096;
unsafe extern "C" { pub static trampoline: u8; pub fn user_vector(); pub fn user_return(); }
pub fn kernel_satp() -> usize {
    let x;
    // SAFETY: 在内核页表激活后读取 satp。
    unsafe { core::arch::asm!("csrr {}, satp", out(reg) x); } x
}
/// # Safety
/// 用户页表映射 trampoline 和 trapframe，frame 的内核字段及通用寄存器均已初始化。
// TODO(lab-4): 保持中断关闭，同步指令缓存，设置高地址 user_vector 为 stvec，
// 写 sepc，准备 SPP/SPIE/SIE，经高地址 user_return(TRAPFRAME, satp) 返回。
// 学生完成准备；底层 CSR 操作与 trampoline 汇编由教师提供。
// SAFETY 要求：转换后的入口必须位于共同映射的 trampoline 页，切换后只访问用户页表内 frame。
pub unsafe fn return_to_user(_satp: usize, _pc: usize) -> ! { todo!("lab-4: return_to_user") }
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
