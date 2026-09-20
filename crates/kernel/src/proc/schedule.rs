use crate::lock::SpinGuard;
use oslab_hal::{arch::context::Context, platform::NCPU};
pub static mut CONTEXTS: [Context; NCPU] = [const { Context::ZERO }; NCPU];
// TODO(lab-6): 开中断轮询，逐槽持锁选 Runnable，置 Running/current；handoff 后切换。
// 返回时用当前 CPU 的锁责任 resume，清 current 并 drop。切换前结束全部可变借用。
pub fn scheduler() -> ! { todo!("lab-6: scheduler") }
// TODO(lab-6): guard 是当前进程唯一锁、关中断、状态非 Running。消耗守卫 handoff 后切回调度器；
// 恢复时重新读取当前 CPU，resume 取得新守卫返回。旧 guard/可变进程借用不得跨切换。
// 用 cpu::resume_interrupts 保存逻辑调用者的恢复策略；接管后 set_resume_interrupts 恢复该布尔策略。
// depth/owner 留在各 CPU，不复制旧 CPU 嵌套状态；trap 继续保持关中断。
pub fn sched<'a>(_guard: SpinGuard<'a>) -> SpinGuard<'a> { todo!("lab-6: sched") }
// TODO(lab-6): 持自身锁置 Runnable，sched 交接后释放返回的新守卫。
pub fn yield_cpu() { todo!("lab-6: yield_cpu") }
// TODO(lab-6): 先取进程锁再释放 condition，同锁时不能重复获取；发布 chan/Sleeping，sched。
// 恢复后清 chan 并重新获取条件锁；返回的新守卫来自当前 CPU。
pub fn sleep<'a>(_chan: usize, _condition: SpinGuard<'a>) -> SpinGuard<'a> { todo!("lab-6: sleep") }
// TODO(lab-6): 逐槽持锁唤醒同 chan 的 Sleeping 进程，跳过自身；改变条件和唤醒须持条件锁。
pub fn wakeup(_chan: usize) { todo!("lab-6: wakeup") }
// TODO(lab-6): 当前核接管调度器交来的进程锁 resume，drop 后进入用户态。
pub extern "C" fn first_return() -> ! { todo!("lab-6: first_return") }

// TODO(lab-7): first_return 中，proczero 交接并释放进程锁后、进入用户态前单次 fs::init；可睡眠，失败停止。

// TODO(lab-9): 首进程 fs 初始化后调用 files 初始化任务，cwd=root，依次打开 stdin/stdout/stderr。
