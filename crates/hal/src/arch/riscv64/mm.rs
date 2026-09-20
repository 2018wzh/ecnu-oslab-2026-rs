//! Sv39 页表项和切换接口。
//! Sv39：VA 的 VPN[2]/VPN[1]/VPN[0]/offset 分别占 9/9/9/12 位。
//! 一页存 4096/8=512 个 PTE。PTE：高 10 位本章清零、44 位 PPN、
//! 2 位 RSW、D/A/G/U/X/W/R/V；V 有效，U 用户，G 全局，A 已访问，D 已写入。
//! V=1 且 R/W/X=0 是指向下一级的非叶项，不是页表内存不可读写。
//! W=1 要求 R=1；本章只在 level 0 建立 4KiB 叶项。
//! satp：MODE[63:60]=8，ASID[59:44]=0，PPN[43:0]=根物理地址>>12。
//! ASID 是地址空间标识，不是 Flash 刷新；切换后须刷新本核翻译缓存。
pub const PAGE_SIZE: usize = 4096;
pub const VA_MAX: usize = 1 << 38;
pub const V: usize = 1;
pub const R: usize = 2;
pub const W: usize = 4;
pub const X: usize = 8;
pub const U: usize = 16;
pub const G: usize = 32;
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
