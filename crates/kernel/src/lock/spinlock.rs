//! 自旋锁。守卫只能在获取锁的 CPU 上释放。
use core::{marker::PhantomData, sync::atomic::{AtomicBool, AtomicUsize, Ordering}};
// locked 表示占用状态；owner 是持有者 cpuid，usize::MAX 表示无持有者。
pub struct SpinLock { locked: AtomicBool, owner: AtomicUsize, handoff_pending: AtomicBool }
pub struct SpinGuard<'a> { lock: &'a SpinLock, _local: PhantomData<*mut ()> }
impl<'a> SpinGuard<'a> {
    /// 消耗普通守卫并显式移交释放责任；不解锁，不移动 CPU 中断状态。
    /// # Safety
    /// 仅在进程/调度器切换边界调用；唯一持锁且关中断，无存活可变进程借用。
    /// 接收执行流必须在该锁上恰好 resume 一次；此后不可取消切换或再次释放旧责任。
    pub unsafe fn handoff(self) {
        self.lock.check_switch_owner();
        assert!(!self.lock.handoff_pending.swap(true, Ordering::Relaxed), "duplicate handoff");
        core::mem::forget(self);
    }
    pub fn lock_ref(&self) -> &'a SpinLock { self.lock }
}
impl SpinLock {
    pub const UNINIT: Self = Self { locked: AtomicBool::new(false), owner: AtomicUsize::new(usize::MAX), handoff_pending: AtomicBool::new(false) };
    fn check_switch_owner(&self) {
        assert!(!oslab_hal::arch::csr::irq_enabled(), "handoff: interrupts enabled");
        assert_eq!(oslab_hal::arch::cpu::interrupt_depth(), 1, "handoff: lock nesting");
        assert!(self.locked.load(Ordering::Relaxed), "handoff: unlocked");
        assert_eq!(self.owner.load(Ordering::Relaxed), oslab_hal::arch::cpu::cpu_id(), "handoff: wrong CPU");
    }
    /// 接管当前 CPU 持有的锁责任，不取回旧 CPU 的守卫或中断嵌套。
    /// # Safety
    /// 只能由匹配 handoff 的接收执行流调用；该锁及其地址稳定且无别的活守卫。
    pub unsafe fn resume(&self) -> SpinGuard<'_> {
        self.check_switch_owner();
        assert!(self.handoff_pending.swap(false, Ordering::Relaxed), "resume without handoff");
        SpinGuard { lock: self, _local: PhantomData }
    }
    /// # Safety
    /// 初始化期间不得有其他执行流使用此锁。
    // TODO(lab-1): 初始化未持有的锁；handoff_pending 也须为 false。
    pub unsafe fn init(&self) { todo!("lab-1: SpinLock::init") }
    // TODO(lab-1): 判断当前 CPU 是否持有锁。
    pub fn holding(&self) -> bool { todo!("lab-1: SpinLock::holding") }
    // TODO(lab-1): push_off 后原子获取锁，记录持有者并返回守卫。
    pub fn lock(&self) -> SpinGuard<'_> { todo!("lab-1: SpinLock::lock") }
}
impl Drop for SpinGuard<'_> {
    // TODO(lab-1): 检查本核持有者及 handoff_pending=false，发布写入，解锁并 pop_off。
    fn drop(&mut self) { todo!("lab-1: SpinGuard::drop") }
}
