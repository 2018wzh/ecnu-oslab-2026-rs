//! 自旋锁。守卫只能在获取锁的 CPU 上释放。
use core::{marker::PhantomData, sync::atomic::{AtomicBool, AtomicUsize}};
pub struct SpinLock { locked: AtomicBool, owner: AtomicUsize }
pub struct SpinGuard<'a> { lock: &'a SpinLock, _local: PhantomData<*mut ()> }
impl SpinLock {
    pub const UNINIT: Self = Self { locked: AtomicBool::new(false), owner: AtomicUsize::new(usize::MAX) };
    /// # Safety
    /// 初始化期间不得有其他执行流使用此锁。
    // TODO(lab-1): 初始化未持有的锁。
    pub unsafe fn init(&self) { todo!("lab-1: SpinLock::init") }
    // TODO(lab-1): 判断当前 CPU 是否持有锁。
    pub fn holding(&self) -> bool { todo!("lab-1: SpinLock::holding") }
    // TODO(lab-1): push_off 后原子获取锁，记录持有者并返回守卫。
    pub fn lock(&self) -> SpinGuard<'_> { todo!("lab-1: SpinLock::lock") }
}
impl Drop for SpinGuard<'_> {
    // TODO(lab-1): 检查持有者，发布写入，解锁并 pop_off。
    fn drop(&mut self) { todo!("lab-1: SpinGuard::drop") }
}
