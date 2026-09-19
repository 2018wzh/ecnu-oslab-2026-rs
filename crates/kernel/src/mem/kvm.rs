use super::*;
pub static mut ROOT: PageTable = core::ptr::null_mut();
unsafe extern "C" { pub static kernel_end: u8; pub static text_end: u8; pub static rodata_end: u8; }
/// # Safety
/// root 是有效页表，调用者独占修改权限。
// TODO(lab-2): 遍历三级页表，必要时分配中间页表。
pub unsafe fn walk(_root: PageTable, _va: usize, _alloc: bool) -> Option<*mut usize> { todo!("lab-2: walk") }
/// # Safety
/// root 有效且可独占修改，物理范围属于调用者。
// TODO(lab-2): 建立 4KiB 映射，检查范围、权限、重复映射和分配失败。
pub unsafe fn map(_root: PageTable, _va: usize, _pa: usize, _len: usize, _perm: usize) -> Result<(), ()> { todo!("lab-2: map") }
/// # Safety
/// root 可独占修改，释放页面时不存在其他引用。
// TODO(lab-2): 清除叶 PTE，可选归还普通池页面。
pub unsafe fn unmap(_root: PageTable, _va: usize, _len: usize, _free_pages: bool) { todo!("lab-2: unmap") }
// TODO(lab-2): 建立代码 RX、只读区 R、数据与可分配区 RW、UART RW 的映射。
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
            crate::println!("level={} index={} pa={:#x} flags={:#x}", depth, index, pte_to_pa(pte), pte & 1023);
            if depth > 0 && pte & (R | W | X) == 0 {
                // SAFETY: 有效非叶 PTE 指向下一级页表，物理内存恒等映射。
                unsafe { level(pte_to_pa(pte) as PageTable, depth - 1); }
            }
        }
    }
    // SAFETY: 继承调用者对页表完整性的保证。
    unsafe { level(root, 2); }
}
