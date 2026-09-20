use crate::{proc::Proc, mem::PageTable};
use super::mmap::Region;
pub const MMAP_END: usize = crate::proc::USER_STACK_TOP - 4096 * 4096;
pub const MMAP_BEGIN: usize = MMAP_END - 16384 * 4096;
/// 教师连续页复制辅助，不选择区域，不复制特殊页或 mmap 节点。
/// # Safety
/// source 及普通页稳定存活；target 独占、未发布且与源无共享页面，目标区间未映射。
/// 区间连续映射用户页，非法参数和底层失败 panic。
pub unsafe fn copy_range(source: PageTable, target: PageTable, begin: usize, end: usize) {
    use super::{kvm, pmem, V, U, R, W, X, G, A, D};
    assert!(!source.is_null() && !target.is_null() && source != target && begin <= end
        && end <= oslab_hal::arch::trap::TRAPFRAME && begin % 4096 == 0 && end % 4096 == 0);
    for va in (begin..end).step_by(4096) {
        // SAFETY: 源稳定，目标独占；新普通页只移交目标，物理内存恒等映射。
        unsafe {
            let pte = *kvm::getpte(source, va, false).expect("copy source");
            assert!(pte & V != 0 && pte & U != 0 && pte & (R | W | X) != 0);
            let page = pmem::alloc(false);
            core::ptr::copy_nonoverlapping(super::pte_to_pa(pte) as *const u8, page as *mut u8, 4096);
            kvm::mappages(target, va, page, 4096, pte & (R | W | X | U | G | A | D));
        }
    }
}
// TODO(lab-5): 手动查询用户页表，支持非对齐和跨页；非法输入可断言或 panic。
pub fn copy_from_user(_p: &Proc, _dst: &mut [u8], _src: usize) { todo!("lab-5: copy_from_user") }
// TODO(lab-5): 逐页复制到可写用户内存，不构造未经校验的用户引用。
pub fn copy_to_user(_p: &Proc, _dst: usize, _src: &[u8]) { todo!("lab-5: copy_to_user") }
// TODO(lab-5): dst.len() 为 maxlen；最多复制这些字节，遇 NUL 提前结束；不强行补 NUL。
// 达到上限不新增错误契约，调用者打印前须确认 NUL 或使用有界输出。
pub fn copy_str_from_user(_p: &Proc, _dst: &mut [u8], _src: usize) { todo!("lab-5: copy_str_from_user") }
/// # Safety
/// root 及待增长地址空间由调用者独占，top/len 页对齐且范围合法。
// TODO(lab-5): 独立堆增长，普通池分配并映射 RWU，返回新堆顶，不超过 MMAP_BEGIN。
pub unsafe fn heap_grow(_root: PageTable, _top: usize, _len: usize) -> usize { todo!("lab-5: heap_grow") }
/// # Safety
/// root 独占；解除区间内没有存活引用，top/len 页对齐且范围合法。
// TODO(lab-5): 独立堆收缩，解除映射并回收普通页，返回新堆顶，不低于 0x2000。
pub unsafe fn heap_ungrow(_root: PageTable, _top: usize, _len: usize) -> usize { todo!("lab-5: heap_ungrow") }
// TODO(lab-5): 由 ustack_npage 推导栈底，合法缺页补齐到 fault 页，更新页数；非法地址 panic。
// 预留 4096 页，不越过 MMAP_END；只增长不收缩，调用者重试原 PC。
pub fn stack_grow(_p: &mut Proc, _fault: usize) { todo!("lab-5: stack_grow") }
/// # Safety
/// head 链稳定存活，遍历期间不能回收节点。
// TODO(lab-5): 在 mmap 区扫描有序已分配链，首次适配 len 字节；找不到返回 0。
pub unsafe fn mmap_find(_head: *const Region, _len: usize) -> usize { todo!("lab-5: mmap_find") }
// TODO(lab-5): begin=0 调用 mmap_find；有序插入、合并相邻区域、普通池申请并映射 RWU。
// syscall 检查字节长度和地址，底层失败 panic，不要求回滚。
pub fn mmap(_p: &mut Proc, _begin: usize, _len: usize) -> usize { todo!("lab-5: uvm::mmap") }
// TODO(lab-5): 裁剪、拆分、跨节点解除并回收普通页和空节点；底层失败 panic。
pub fn munmap(_p: &mut Proc, _begin: usize, _len: usize) { todo!("lab-5: uvm::munmap") }
/// # Safety
/// source 和 mmap 链稳定存活，target 是独立、未发布的根；不共享可写所有权。
// TODO(lab-5): 按代码、堆、栈、mmap 区域调用 copy_range，保留权限与空洞。
// 不复制 frame/trampoline、进程字段或 mmap 节点（留待 lab-6）。
pub unsafe fn copy_pgtbl(_source: PageTable, _target: PageTable, _heap_top: usize, _ustack_npage: usize, _mmap: *const Region) {
    todo!("lab-5: copy_pgtbl")
}
// TODO(lab-5): 递归回收普通池用户叶页与内核池各级页表页；顶级 level=3，跳过无效项。
unsafe fn destroy_table(_root: PageTable, _level: usize) { todo!("lab-5: destroy_table") }
/// 教师销毁入口：内核池独占 frame 解除并释放，共享 trampoline 仅解除。
/// # Safety
/// root 为空或已停止使用，调用者独占普通页、frame 和页表页，无存活引用。
/// 返回后原 frame 指针失效，调用方必须清除它，不得再次释放。
pub unsafe fn destroy(root: PageTable) {
    use oslab_hal::arch::trap::{TRAPFRAME, TRAMPOLINE};
    if root.is_null() { return; }
    // SAFETY: 查询现有映射；先解除后回收 frame，避免传给普通池释放路径。
    unsafe {
        for va in [TRAPFRAME, TRAMPOLINE] {
            if let Some(pte) = super::kvm::getpte(root, va, false) {
                if *pte & super::V != 0 {
                    let pa = super::pte_to_pa(*pte);
                    super::kvm::unmappages(root, va, 4096, false);
                    if va == TRAPFRAME { super::pmem::free(pa, true); }
                }
            }
        }
        destroy_table(root, 3);
    }
}
