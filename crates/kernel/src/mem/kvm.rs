use super::*;
pub static mut ROOT: PageTable = core::ptr::null_mut();
unsafe extern "C" { pub static kernel_end: u8; pub static text_end: u8; pub static rodata_end: u8; }
/// # Safety
/// root 是有效页表，调用者独占修改权限。
// TODO(lab-2): 遍历三级页表，va < VA_MAX；不创建且路径缺失返回 None。
// alloc 为真时从内核池分配中间页表，耗尽 panic；只支持 4KiB 叶项。
pub unsafe fn getpte(_root: PageTable, _va: usize, _alloc: bool) -> Option<*mut usize> { todo!("lab-2: getpte") }
/// # Safety
/// root 有效且可独占修改，物理范围属于调用者。
// TODO(lab-2): 建立 4KiB 映射，错误 panic；va/pa 页对齐，len > 0，末页向上覆盖。
// 加法及取整不得溢出，覆盖范围不得超过 VA_MAX 或 56 位物理地址范围。
// perm 仅含 R/W/X/U/G/A/D，R 或 X 至少一个，W 必须同时有 R；叶项设置 V/A/D。
// 同一 VA/PA 可更新权限，改指向另一 PA 则 panic。
pub unsafe fn mappages(_root: PageTable, _va: usize, _pa: usize, _len: usize, _perm: usize) { todo!("lab-2: mappages") }
/// # Safety
/// root 可独占修改，释放页面时不存在其他引用。
// TODO(lab-2): va 页对齐、len > 0，末页向上覆盖，范围与溢出约束同映射。
// 解除缺失映射 panic；清除叶项，可选归还普通池数据页，不回收中间页表。
pub unsafe fn unmappages(_root: PageTable, _va: usize, _len: usize, _free_pages: bool) { todo!("lab-2: unmappages") }
// TODO(lab-2): 建立代码 RX、只读区 R、数据与可分配区 RW、UART 和 PLIC RW 的映射。
// 不设置 U，不映射固件保留区；CLINT 由固件负责，本章不实现中断驱动。
pub fn init() { todo!("lab-2: kvm::init") }
pub fn init_hart() {
    // SAFETY: 主核发布初始化结果后，每核调用；ROOT 包含代码和每核栈。
    unsafe { oslab_hal::arch::mm::activate(ROOT); }
}
/// # Safety
/// root 及它引用的页表有效，且遍历期间不被修改。
pub unsafe fn print(root: PageTable) {
    unsafe fn level(root: PageTable, depth: usize) {
        for index in 0..512 {
            // SAFETY: root 指向含 512 项的页表。
            let pte = unsafe { *root.add(index) };
            if pte & V == 0 { continue; }
            for _ in depth..3 { crate::print!(".. "); }
            crate::println!("level={} index={} pa={:#x} flags={:#x}", depth, index, pte_to_pa(pte), pte & 1023);
            if depth > 0 && pte & (R | W | X) == 0 {
                // SAFETY: 有效非叶 PTE 指向下一级页表，物理内存恒等映射。
                unsafe { level(pte_to_pa(pte) as PageTable, depth - 1); }
            }
        }
    }
    crate::println!("root pgtbl: pa={:p}", root);
    // SAFETY: 继承调用者对页表完整性的保证。
    unsafe { level(root, 2); }
}
