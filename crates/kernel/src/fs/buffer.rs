use crate::lock::{SpinLock, sleeplock::SleepLock};
pub const N_BUFFER_TEST: usize = 8;
pub const N_BUFFER: usize = 16384;
pub const UNUSED: u32 = u32::MAX;
pub struct Buffer { pub block: u32, pub refs: usize, pub valid: bool, pub disk: bool, pub io_result: i32,
    pub data: *mut u8, pub lock: SleepLock, pub prev: *mut Buffer, pub next: *mut Buffer }
impl Buffer {
    pub const EMPTY: Self = Self { block: UNUSED, refs: 0, valid: false, disk: false, io_result: 0,
        data: core::ptr::null_mut(), lock: SleepLock::UNINIT, prev: core::ptr::null_mut(), next: core::ptr::null_mut() };
}
pub static CACHE_LOCK: SpinLock = SpinLock::UNINIT;
pub static mut CACHE: [Buffer; N_BUFFER] = [const { Buffer::EMPTY }; N_BUFFER];
pub static mut ACTIVE: Buffer = Buffer::EMPTY;
pub static mut INACTIVE: Buffer = Buffer::EMPTY;
/// 教师链表辅助：只维护链接，不改变引用数或内容。
/// # Safety
/// 调用者持有 CACHE_LOCK；两哨兵已初始化，node 是 CACHE 中的有效节点，
/// 链接要么均为空，要么属于有效循环链表，且不得存在别名引用。
pub unsafe fn move_node(node: *mut Buffer, active: bool, front: bool) {
    assert!(CACHE_LOCK.holding());
    assert!(!node.is_null() && node != &raw mut ACTIVE && node != &raw mut INACTIVE);
    unsafe {
        assert_eq!((*node).next.is_null(), (*node).prev.is_null());
        if !(*node).next.is_null() {
            (*(*node).next).prev = (*node).prev;
            (*(*node).prev).next = (*node).next;
        }
        let head = if active { &raw mut ACTIVE } else { &raw mut INACTIVE };
        let left = if front { head } else { (*head).prev };
        let right = if front { (*head).next } else { head };
        (*node).prev = left;
        (*node).next = right;
        (*left).next = node;
        (*right).prev = node;
    }
}

/// data 指针发布/清空也需 CACHE_LOCK；页内容和 valid 仍由睡眠锁保护。
/// 仅测试配置取最多八行快照，避免内核栈溢出。
pub fn print_info() {
    if N_BUFFER != N_BUFFER_TEST { crate::println!("buffer detail requires N_BUFFER_TEST=8"); return; }
    let mut rows = [(0usize, 0u32, 0usize, false, 0usize); N_BUFFER_TEST];
    let mut count = 0;
    let guard = CACHE_LOCK.lock();
    // SAFETY: 初始化后的链表及 block/refs 均由 CACHE_LOCK 保护。
    unsafe {
        for active in [true, false] {
            let head = if active { &raw mut ACTIVE } else { &raw mut INACTIVE };
            let mut node = (*head).next;
            while node != head {
                let base = (&raw mut CACHE).cast::<Buffer>();
                let index = (0..N_BUFFER).find(|&i| node == base.add(i)).expect("buffer list node");
                assert!(count < N_BUFFER_TEST, "buffer list cycle");
                rows[count] = (index, (*node).block, (*node).refs, active, (*node).data as usize);
                count += 1;
                node = (*node).next;
            }
        }
    }
    drop(guard);
    crate::println!("buffer cache (head to tail):");
    for &(index, block, refs, active, data) in &rows[..count] {
        crate::println!("{} buffer {}(ref = {}): page(pa = {:#x}) -> block[{}]",
            if active { "active" } else { "inactive" }, index, refs, data, block);
    }
}
pub struct BufferGuard { buffer: *mut Buffer, lock: Option<crate::lock::sleeplock::SleepGuard<'static>> }
impl BufferGuard {
    pub fn data(&self) -> &[u8] {
        // SAFETY: get 保证页有效且守卫独占内容，生命周期不超过守卫。
        unsafe { core::slice::from_raw_parts((*self.buffer).data, super::BLOCK_SIZE) }
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        // SAFETY: 守卫持有睡眠锁，独占数据页。
        unsafe { core::slice::from_raw_parts_mut((*self.buffer).data, super::BLOCK_SIZE) }
    }
    // TODO(lab-7): 检查睡眠锁并同步读盘，与 write 独立。
    pub fn read(&mut self) { todo!("lab-7: BufferGuard::read") }
    pub fn token(&self) -> usize { self.buffer as usize }
    // TODO(lab-7): 同步写盘，失败沿用 panic 契约。
    pub fn write(&mut self) { todo!("lab-7: BufferGuard::write") }
}
impl Drop for BufferGuard {
    // TODO(lab-7): 先释放睡眠锁，再减 refs；归零移入 inactive 头部。
    fn drop(&mut self) { todo!("lab-7: BufferGuard::drop") }
}
// TODO(lab-7): 初始化两条哨兵双向链表、锁和引用数，数据页按需分配。
pub fn init() { todo!("lab-7: buffer::init") }
// TODO(lab-7): 查找/引用/LRU 选择在全局锁下；解锁后获取睡眠锁，miss 读盘。
pub fn get(_block: u32) -> Result<BufferGuard, ()> { todo!("lab-7: buffer::get") }
// TODO(lab-7): 仅释放 inactive 尾部无引用/无 I/O 的数据页并清 valid。
pub fn freemem(_count: usize) -> usize { todo!("lab-7: buffer::freemem") }
