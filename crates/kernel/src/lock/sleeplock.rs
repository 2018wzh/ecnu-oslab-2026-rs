use super::SpinLock;
use core::{cell::UnsafeCell, marker::PhantomData};
pub struct SleepLock { lock: SpinLock, state: UnsafeCell<(bool, usize)> }
// SAFETY: state 的所有读写必须由内部 SpinLock 串行化。
unsafe impl Sync for SleepLock {}
pub struct SleepGuard<'a> { lock: &'a SleepLock, _local: PhantomData<*mut ()> }
impl SleepLock {
    pub const UNINIT: Self = Self { lock: SpinLock::UNINIT, state: UnsafeCell::new((false, 0)) };
    /// # Safety
    /// 初始化时无人访问该锁。
    // TODO(lab-6): 初始化内部锁及状态。
    pub unsafe fn init(&self) { todo!("lab-6: SleepLock::init") }
    // TODO(lab-6): 内部锁保护下判断当前进程所有权。
    pub fn holding(&self) -> bool { todo!("lab-6: SleepLock::holding") }
    // TODO(lab-6): 循环检查条件，使用 schedule::sleep 等待。
    pub fn lock(&self) -> SleepGuard<'_> { todo!("lab-6: SleepLock::lock") }
}
impl Drop for SleepGuard<'_> {
    // TODO(lab-6): 验证所有者，清状态并唤醒等待者。
    fn drop(&mut self) { todo!("lab-6: SleepGuard::drop") }
}
