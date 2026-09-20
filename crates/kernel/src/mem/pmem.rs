//! 用空闲链表管理 4KiB 页面；池对象和字段仅在此模块内访问。
use crate::lock::SpinLock;
pub const KERNEL_PAGES: usize = 1024;
/// 空闲页首字存放 next 地址；0 为空。分配后整页归调用者。
#[repr(C)]
struct PageNode { next: usize }
struct Region {
    begin: usize, end: usize, // 页对齐半开区间，初始化发布后不变。
    lk: SpinLock,            // 保护 allocable、哨兵及所有空闲页 next 链接。
    allocable: u32,          // 当前可分配页数。
    list_head: PageNode,     // 常驻哨兵，本身不是可分配页。
}
impl Region {
    const EMPTY: Self = Self { begin: 0, end: 0, lk: SpinLock::UNINIT,
        allocable: 0, list_head: PageNode { next: 0 } };
}
static mut KERN_REGION: Region = Region::EMPTY;
static mut USER_REGION: Region = Region::EMPTY;
// 访问约束：用 &raw mut 获取池指针，仅为 lk 创建共享引用并持有 RAII 守卫；
// 持锁后通过原始指针修改其他字段，不为整个静态 Region 创建 &mut。
// 初始化只由主核执行一次，发布之前没有并发访问。
// TODO(lab-2): 从页对齐的 kernel_end 到 DRAM_BASE + DRAM_SIZE 划分两池，
// 前 1024 页供内核，其余供普通页面；填写边界、锁、计数和哨兵空闲链表。
// 可以自行提取私有辅助函数，不要求独立公开的建链接口。
pub fn init() { todo!("lab-2: pmem::init") }
// TODO(lab-2): 持对应池锁摘链并更新计数，返回清零页地址；失败（包括耗尽）panic。
// 摘出的页已独占，可在解锁后清零。
pub fn alloc(_kernel: bool) -> usize { todo!("lab-2: pmem::alloc") }
/// # Safety
/// pa 是指定池分配的页面，调用者停止所有使用，消除引用及需要保持有效的别名映射。
// TODO(lab-2): 释放物理页，持对应池锁归还链表并更新计数；失败 panic。
pub unsafe fn free(_pa: usize, _kernel: bool) { todo!("lab-2: pmem::free") }
