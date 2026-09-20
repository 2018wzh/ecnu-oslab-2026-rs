use super::{Proc, N_PROC};
use crate::lock::{SpinLock, SpinGuard};
pub static mut TABLE: [Proc; N_PROC] = [const { Proc::EMPTY }; N_PROC];
static PID_LOCK: SpinLock = SpinLock::UNINIT;
static mut NEXT_PID: u32 = 1;
/// 教师 PID 辅助：独立锁，耗尽诊断并 panic，避免有符号溢出。
pub fn alloc_pid() -> usize {
    let _guard = PID_LOCK.lock();
    // SAFETY: PID_LOCK 串行化唯一计数器。
    unsafe {
        assert!(NEXT_PID <= i32::MAX as u32, "PID exhausted: next={}", NEXT_PID as usize);
        let pid = NEXT_PID as usize;
        NEXT_PID += 1;
        pid
    }
}
/// # Safety
/// 启动期间独占调用，尚无并发分配。
pub unsafe fn pid_init() {
    // SAFETY: 启动独占初始化。
    unsafe { PID_LOCK.init(); NEXT_PID = 1; }
}
// TODO(lab-6): 主核初始化 32 槽及每槽锁、PROCZERO=null，并调用 pid_init；发布后不重置。
pub fn init() { todo!("lab-6: lifecycle::init") }
// TODO(lab-6): 逐槽持锁选 Unused 且 !reclaiming；初始化 pid、内核池清零的独立 frame/页表，按槽索引设置常驻 kstack。
// context 入口 first_return、sp 为对齐栈顶；返回仍持锁，完成初始化后才发布 Runnable；无槽 None。
// SAFETY 要求：仅从原始指针投影 lock 的共享引用，不能同时构造覆盖 lock 的 &mut Proc。
pub fn slot_alloc() -> Option<(*mut Proc, SpinGuard<'static>)> { todo!("lab-6: slot_alloc") }
/// # Safety
/// p 已停止运行且调用者持有其锁；只修改锁以外字段，不构造覆盖锁引用的 &mut Proc。
// TODO(lab-6): uvm::destroy 释放 frame 一次，清指针；回收 mmap 节点及普通资源，保留栈/锁，置 Unused。
pub unsafe fn free(_p: *mut Proc, _guard: SpinGuard<'static>) -> SpinGuard<'static> { todo!("lab-6: lifecycle::free") }
// TODO(lab-6): 无槽 -1；uvm::copy_pgtbl 深复制普通页；独立复制 mmap 节点、frame、进程字段。
// trampoline 共享；底层耗尽 panic，不要求回滚。子 frame 从父 ecall 状态复制，HAL return_value(0)
// 恰好推进子 PC 一次；父返回子 pid，用户 trap 再推进父 PC 一次。设置 parent，发布 Runnable 并解锁。
pub fn fork() -> isize { todo!("lab-6: fork") }
/// # Safety
/// parent 是当前进程；父子锁序与关系访问遵守 README。
// TODO(lab-6): 将全部孩子托孤给 PROCZERO，唤醒需要回收的根。
pub unsafe fn reparent(_parent: *mut Proc) { todo!("lab-6: reparent") }
/// # Safety
/// p 有效且调用者持 p 的锁。
// TODO(lab-6): 仅唤醒等待自身通道的 Sleeping 父进程。
pub unsafe fn try_wakeup(_p: *mut Proc) { todo!("lab-6: try_wakeup") }
// TODO(lab-6): 禁止根退出；reparent，记录状态，唤醒父亲，置 Zombie、释放父锁后仅持自身锁 sched。
pub fn exit(_status: i32) -> ! { todo!("lab-6: exit") }
// TODO(lab-6): 持父锁原子预筛 parent，只对匹配的孩子取锁并复核，无子 -1；Zombie 的 status!=0 才 copy_to_user i32 状态。
// 先复制再回收并返回 pid；非法复制沿用 lab-5 panic，不要求重试。未等到则睡眠。
pub fn wait(_status: usize) -> isize { todo!("lab-6: wait") }

// TODO(lab-9): init/slot_alloc 初始化 files/cwd 为空和 reclaiming=false；选槽必须同时满足 UNUSED/Unused 且 !reclaiming。
// fork 增加所有 file/cwd 引用；首进程 first_return 解锁并 fs_init 后设置 cwd=root 和 fd0/1/2。
// proc_free/free 两阶段：只持目标进程锁，标记 reclaiming 并移出 files/cwd；释放该锁，关闭移出的引用；
// 关闭后如需清除已发布 parent 关系，先取父锁再取目标锁；未发布槽只取目标锁。
// 完成其余资源回收、清 parent、置 UNUSED/Unused 并清标记；释放父锁，返回仍持目标锁。不能在锁内关闭/Drop。
// wait 持父/子锁复核 Zombie 且 !reclaiming，复制退出码并记住 pid；释放父锁后调用 free；
// 其他等待者跳过 reclaiming 槽且不得视其为无子；完成后唤醒等待父进程，遵守父->子锁序。
// 分配失败清理也只持目标锁使用同一协议；其他分配者不能复用回收中的槽。
// exit 只发布 Zombie；文件/cwd 的释放在 free 阶段，不提前在 exit 关闭。
