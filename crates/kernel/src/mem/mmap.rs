use crate::lock::SpinLock;
pub const N_MMAP: usize = 256;
pub struct Region { pub begin: usize, pub pages: usize, pub next: *mut Region }
impl Region { pub const EMPTY: Self = Self { begin: 0, pages: 0, next: core::ptr::null_mut() }; }
// 外层仓库链接与内层进程区域链接分开，头节点不可分配。
struct Node { region: Region, next: *mut Node }
impl Node { const EMPTY: Self = Self { region: Region::EMPTY, next: core::ptr::null_mut() }; }
static mut NODE_LIST: [Node; N_MMAP] = [const { Node::EMPTY }; N_MMAP];
static mut LIST_HEAD: Node = Node::EMPTY;
static LIST_LOCK: SpinLock = SpinLock::UNINIT;

/// 输出可用的仓库节点链，供调试；持锁遍历打印；锁顺序为节点池锁 -> 打印锁，禁止反向获取。
/// 须在节点池初始化并发布后调用。
pub fn print_free() {
    let mut count = 0;
    let guard = LIST_LOCK.lock();
    // SAFETY: 仓库链接由 LIST_LOCK 保护；先核对数组成员身份再解引用，
    // 不创建覆盖已借出 Region 的引用。
    let valid = unsafe {
        let base = core::ptr::addr_of_mut!(NODE_LIST).cast::<Node>();
        let mut node = LIST_HEAD.next;
        while !node.is_null() && count < N_MMAP {
            let Some(index) = (0..N_MMAP).find(|&i| node == base.add(i)) else { break; };
            crate::println!("node {} index = {}", count, index);
            count += 1;
            node = (*node).next;
        }
        node.is_null()
    };
    drop(guard);
    assert!(valid, "invalid mmap free list");
}
/// 教师区域合并辅助，不修改 next，不操作用户页面。
/// 非法/不相邻返回 Err 且不修改；预期合并失败时调用者 panic。
/// # Safety
/// 两个节点由调用者独占且无其他借用；成功后 discard 失效，调用者须修复链表。
pub unsafe fn merge(left: *mut Region, right: *mut Region, keep_left: bool) -> Result<*mut Region, ()> {
    use super::uvm::{MMAP_BEGIN, MMAP_END};
    if left.is_null() || right.is_null() || left == right { return Err(()); }
    // SAFETY: 调用者拥有两个有效节点；不为随后归还的节点建立存活引用。
    unsafe {
        if (*left).pages == 0 || (*right).pages == 0
            || (*left).begin < MMAP_BEGIN || (*right).begin >= MMAP_END
            || (*left).begin % 4096 != 0 || (*right).begin % 4096 != 0
            || (*left).begin >= (*right).begin || (*left).pages != ((*right).begin - (*left).begin) / 4096
            || (*right).pages > (MMAP_END - (*right).begin) / 4096 { return Err(()); }
        let (keep, discard) = if keep_left { (left, right) } else { (right, left) };
        let begin = (*left).begin;
        let pages = (*left).pages + (*right).pages;
        (*keep).begin = begin; (*keep).pages = pages;
        free(discard);
        Ok(keep)
    }
}
/// 教师诊断：显示已分配区域，不访问池的内部空闲表示。
/// # Safety
/// head 指向存活、由调用者独占的节点链。
pub unsafe fn print(mut head: *const Region) {
    crate::println!("\nalloced mmap_space:");
    if head.is_null() { crate::println!("empty"); }
    for _ in 0..N_MMAP {
        if head.is_null() { return; }
        // SAFETY: 继承调用者的链表存活与独占约定。
        unsafe {
            crate::println!("mmap begin={:#x} pages={}", (*head).begin, (*head).pages);
            head = (*head).next;
        }
    }
    assert!(head.is_null(), "mmap list cycle or too many nodes");
}
// TODO(lab-5): 按数组索引升序链接空闲节点，初始化不可分配的头节点和保护锁。
pub fn init() { todo!("lab-5: mmap::init") }
// TODO(lab-5): 持锁取出并清空节点，耗尽 panic。
pub fn alloc() -> *mut Region { todo!("lab-5: mmap::alloc") }
/// # Safety
/// node 来自本池且不再被进程引用。
// TODO(lab-5): 验证归属并归还节点；不得假定 Node.region 偏移为零，
// 可用 addr_of! 比较成员地址或 offset_of! 恢复包装节点，不能创建覆盖已借出节点的引用。
pub unsafe fn free(_node: *mut Region) { todo!("lab-5: mmap::free") }
