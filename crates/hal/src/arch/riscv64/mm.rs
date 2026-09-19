//! Sv39 页表项和切换接口。
pub const PAGE_SIZE: usize = 4096;
pub const VA_MAX: usize = 1 << 38;
pub const V: usize = 1;
pub const R: usize = 2;
pub const W: usize = 4;
pub const X: usize = 8;
pub const U: usize = 16;
pub const A: usize = 64;
pub const D: usize = 128;
pub type PageTable = *mut usize;
pub const fn pa_to_pte(pa: usize) -> usize { (pa >> 12) << 10 }
pub const fn pte_to_pa(pte: usize) -> usize { (pte >> 10) << 12 }
pub const fn vpn(va: usize, level: usize) -> usize { (va >> (12 + 9 * level)) & 511 }
/// # Safety
/// root 必须指向完整页表，且映射当前代码、栈和需要访问的数据。
pub unsafe fn activate(root: PageTable) {
    // SAFETY: 调用者保证页表有效；写 satp 后刷新本核 TLB。
    unsafe { core::arch::asm!("csrw satp, {}", "sfence.vma zero, zero", in(reg) (8usize << 60) | (root as usize >> 12)); }
}
