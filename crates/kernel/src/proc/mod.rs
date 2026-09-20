use crate::mem::PageTable;
use oslab_hal::arch::context::Context;
use oslab_hal::{arch::{cpu, trap::UserFrame}, platform::NCPU};
pub const USER_ENTRY: usize = 0x1000;
pub static USER_IMAGE: &[u8] = include_bytes!(env!("OSLAB_USER_IMAGE"));
#[derive(Clone, Copy, PartialEq)]
pub enum State { Unused, Running }
pub struct Proc {
    pub pid: usize,
    pub state: State,
    /// 用户页表根物理地址，内核通过恒等映射访问。
    pub pgtbl: PageTable,
    /// 字节单位；初值 0x2000，伸缩属于 lab-5。
    pub heap_top: usize,
    /// 用户栈页数，初值 1。
    pub ustack_npage: usize,
    /// 内核恒等映射下的独占 frame 页面，不是 TRAPFRAME 用户 VA。
    pub frame: *mut UserFrame,
    /// 高地址虚拟栈底；回收前须查 PTE 得物理地址，不能直接传给 free。
    pub kstack: usize,
    pub context: Context,
}
/// 每个内核栈一页，间隔一页不映射；id 须在有效进程编号范围内。
pub const fn kstack(id: usize) -> usize {
    oslab_hal::arch::trap::TRAPFRAME - (id + 1) * 2 * crate::mem::PAGE_SIZE
}
pub const USER_STACK_TOP: usize = oslab_hal::arch::trap::TRAPFRAME;
/// 每核旧执行流的独立存储；切换期间关闭中断并独占本核槽位。
pub static mut BOOT_CONTEXT: [Context; NCPU] = [const { Context::ZERO }; NCPU];
impl Proc {
    pub const EMPTY: Self = Self { pid: 0, state: State::Unused, pgtbl: core::ptr::null_mut(), heap_top: 0, ustack_npage: 0, frame: core::ptr::null_mut(), kstack: 0, context: Context::ZERO };
}
pub static mut PROCZERO: Proc = Proc::EMPTY;
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
/// frame_pa 是调用者持有且已清零的内核池页面，在页表引用期间不得释放。
// TODO(lab-4): 从内核池申请并清零根页表；映射 trampoline RX、frame RW，均不设 U。
// trampoline 与内核相同 VA/PA；遵循 lab-2 的 getpte/mappages 契约，错误 panic。
pub unsafe fn pgtbl_init(_frame_pa: usize) -> PageTable { todo!("lab-4: proc::pgtbl_init") }
// TODO(lab-4): 仅主核完成内存/中断初始化后创建唯一首进程。
// 设置 pid/state，申请并清零内核池 frame，调用 pgtbl_init。
// 普通池申请并清零镜像/栈页，检查镜像长度，复制平坦镜像。
// 映射 USER_ENTRY 代码数据 RWXU、用户栈 RWU，最低页不映射。
// 设置 heap_top=0x2000、ustack_npage=1、用户 PC/SP；kstack=kstack(0)。
// 栈物理页已由 kvm::init 映射，不重复分配；context 入口为 enter_user，sp 为对齐栈顶。
// 关闭中断后发布 current，用 BOOT_CONTEXT 本核槽保存启动执行流，调用 arch_switch。
// SAFETY 要求：独占 PROCZERO、frame 和本核 context；切换前结束所有可变借用，
// next 的栈/入口须有效；不发布半初始化对象，不引入调度循环。
pub fn make_first() -> ! { todo!("lab-4: proc::make_first") }
