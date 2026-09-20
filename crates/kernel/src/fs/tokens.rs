//! 教师测试适配：保持跨 syscall 的 BufferGuard，不实现缓存算法或用户复制。
//! 只允许原样回传令牌、单次归还，未归还资源不得跨 fork/exit。
use super::buffer::{BufferGuard, N_BUFFER};
use crate::lock::SpinLock;
static LOCK: SpinLock = SpinLock::UNINIT;
static mut SLOTS: [Option<(usize, BufferGuard)>; N_BUFFER] = [const { None }; N_BUFFER];
/// 启动单次调用，尚无令牌。由学生 fs::init 接入。
pub fn init() {
    // SAFETY: 初始化发生在发布用户测试接口之前。
    unsafe { LOCK.init(); }
}
fn owner() -> usize {
    // SAFETY: syscall 中 current 在当前调用期间存活；只复制 pid。
    unsafe { (*crate::proc::current()).pid as usize }
}
fn take(token: usize) -> BufferGuard {
    let _lock = LOCK.lock();
    // SAFETY: 短期独占槽表，不跨锁释放保留引用。
    unsafe {
        let slots = &mut *(&raw mut SLOTS);
        let slot = slots.iter_mut().find(|s| s.as_ref().is_some_and(|(pid, g)| *pid == owner() && g.token() == token))
            .expect("invalid or returned buffer token");
        slot.take().unwrap().1
    }
}
/// 接收 get 的拥有权，返回原内核 buffer 地址；不分配进程资源表。
pub fn retain(guard: BufferGuard) -> usize {
    let token = guard.token();
    let _lock = LOCK.lock();
    // SAFETY: 独占全局适配槽；guard 留在表中，保持睡眠锁和 refs。
    unsafe {
        let slots = &mut *(&raw mut SLOTS);
        let slot = slots.iter_mut().find(|s| s.is_none()).expect("token capacity");
        *slot = Some((owner(), guard));
    }
    token
}
/// 短借用现有令牌。操作可睡眠，但不持有槽表锁，不保留槽表引用。
pub fn with<T>(token: usize, operation: impl FnOnce(&mut BufferGuard) -> T) -> T {
    let mut guard = take(token);
    let result = operation(&mut guard);
    assert_eq!(retain(guard), token);
    result
}
/// put 唯一消费守卫，实际缓存释放职责仍在学生 BufferGuard::drop 中。
pub fn release(token: usize) { drop(take(token)); }
