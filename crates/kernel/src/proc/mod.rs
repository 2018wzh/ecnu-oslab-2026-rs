pub mod lifecycle;
pub mod schedule;
use crate::{mem::PageTable, lock::SpinLock};
pub const N_PROC: usize = 32;
use oslab_hal::arch::context::Context;
use oslab_hal::{arch::{cpu, trap::UserFrame}, platform::NCPU};
pub const USER_ENTRY: usize = 0x1000;
pub static USER_IMAGE: &[u8] = include_bytes!(env!("OSLAB_USER_IMAGE"));
#[derive(Clone, Copy, PartialEq)]
pub enum State { Unused, Zombie, Sleeping, Runnable, Running }
pub struct Proc {
    pub lock: SpinLock,
    pub name: [u8; 16],
    pub parent: core::sync::atomic::AtomicPtr<Proc>,
    pub exit_code: i32,
    pub chan: usize,
    pub pid: usize,
    pub state: State,
    /// 用户页表根物理地址，内核通过恒等映射访问。
    pub pgtbl: PageTable,
    /// 字节单位；初值 0x2000，按页伸缩。
    pub heap_top: usize,
    /// 用户栈页数，初值 1。
    pub ustack_npage: usize,
    /// 内核恒等映射下的独占 frame 页面，不是 TRAPFRAME 用户 VA。
    pub frame: *mut UserFrame,
    /// 高地址虚拟栈底；启动时映射，常驻且随槽复用，进程回收不释放。
    pub kstack: usize,
    pub mmap: *mut crate::mem::mmap::Region,
    pub context: Context,
}
/// 每个内核栈一页，间隔一页不映射；id 是 0..N_PROC 的槽索引，不是 PID。
pub const fn kstack(id: usize) -> usize {
    oslab_hal::arch::trap::TRAPFRAME - (id + 1) * 2 * crate::mem::PAGE_SIZE
}
pub const USER_STACK_TOP: usize = oslab_hal::arch::trap::TRAPFRAME;
impl Proc {
    pub const EMPTY: Self = Self { lock: SpinLock::UNINIT, name: [0; 16], parent: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()), exit_code: 0, chan: 0, pid: 0, state: State::Unused, pgtbl: core::ptr::null_mut(), heap_top: 0, ustack_npage: 0, frame: core::ptr::null_mut(), kstack: 0, mmap: core::ptr::null_mut(), context: Context::ZERO };
}
pub static mut PROCZERO: *mut Proc = core::ptr::null_mut();
static mut CURRENT: [*mut Proc; NCPU] = [core::ptr::null_mut(); NCPU];
pub fn current() -> *mut Proc {
    // SAFETY: 每核仅访问自己的槽位，进程切换期间关闭中断。
    unsafe { CURRENT[cpu::cpu_id()] }
}
/// # Safety
/// p 在作为本核 current 期间保持有效，调用者关闭中断且独占本核槽位。
pub unsafe fn set_current(p: *mut Proc) {
    // SAFETY: 满足调用者的槽位独占约定。
    unsafe { CURRENT[cpu::cpu_id()] = p; }
}
/// # Safety
/// frame_pa 是已清零的内核池页面；映射成功后由用户页表独占，destroy 释放。
// TODO(lab-4): 从内核池申请并清零根页表；映射 trampoline RX、frame RW，均不设 U。
// trampoline 与内核相同 VA/PA；遵循 lab-2 的 getpte/mappages 契约，错误 panic。
pub unsafe fn pgtbl_init(_frame_pa: usize) -> PageTable { todo!("lab-4: proc::pgtbl_init") }
// TODO(lab-6): 主核使用 lifecycle::slot_alloc 得到持锁槽，设置 PROCZERO 指针。
// 保留 lab-4 镜像/用户栈初始化：USER_ENTRY RWXU、用户栈 RWU，最低页不映射。
// 普通池申请并清零镜像页和用户栈页，复制单页镜像；heap_top=0x2000、ustack_npage=1、mmap=null，设置用户 PC/SP。
// 槽分配已准备独立 frame/页表、常驻 kstack 和 first_return context。
// 发布 Runnable 后释放锁并返回，不直接切换或设置 current。
pub fn make_first() { todo!("lab-6: proc::make_first") }
