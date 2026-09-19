//! 用空闲链表管理 4KiB 页面。
use crate::lock::SpinLock;
pub const KERNEL_PAGES: usize = 1024;
pub struct Region { pub begin: usize, pub end: usize, pub free_count: usize, pub head: usize, pub lock: SpinLock }
impl Region {
    pub const EMPTY: Self = Self { begin: 0, end: 0, free_count: 0, head: 0, lock: SpinLock::UNINIT };
}
pub static mut REGIONS: [Region; 2] = [Region::EMPTY, Region::EMPTY];
// TODO(lab-2): 从 kernel_end 划分内核池和普通页面池，初始化锁和链表。
pub fn init() { todo!("lab-2: pmem::init") }
/// # Safety
/// region 指向初始化中的区域，范围内页面尚未分配。
// TODO(lab-2): 将区域内每个页面链接到空闲链表。
pub unsafe fn build_free_list(_region: *mut Region) { todo!("lab-2: build_free_list") }
// TODO(lab-2): 持锁摘取并清零页面；耗尽返回 None。
pub fn alloc(_kernel: bool) -> Option<usize> { todo!("lab-2: pmem::alloc") }
/// # Safety
/// pa 是指定池分配的页面，调用者已消除所有引用和映射。
// TODO(lab-2): 验证对齐、归属和重复释放，再归还页面。
pub unsafe fn free(_pa: usize, _kernel: bool) { todo!("lab-2: pmem::free") }
