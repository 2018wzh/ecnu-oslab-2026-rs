//! 进程控制块与进程表。
//!
//! 进程状态机: Unused -> Runnable -> Running <-> Sleeping -> Zombie (等父回收)。
//! "检查条件"与"改变状态"必须在持锁下做, 否则中间窗口可能被另一个
//! hart 插入 (最典型的是 lost wakeup, 见 [`sleep`] 的说明)。

use core::sync::atomic::{AtomicUsize, Ordering};

use oslab_hal::arch;

use crate::mm::pmem;

// ===========================================================================
// 进程状态与进程表
// ===========================================================================

/// 进程状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    /// 未使用 (进程表里的空槽)。
    Unused,
    /// 可运行, 等待被调度。
    Runnable,
    /// 正在某个 hart 上运行。
    Running,
    /// 睡眠中, 等待某个事件 (被 [`wakeup`] 唤醒)。
    Sleeping,
    /// 已退出, 等待父进程回收 (僵尸)。
    Zombie,
}

/// 最大进程数。
///
/// 用固定进程表而非动态分配: 进程表最需要"一定能查", 固定大小可用
/// 静态数组表达, 于是进程号 = 数组下标在编译期成立。
pub const NPROC: usize = 64;

/// 每个进程的内核栈大小 (4 页 = 16 KiB)。
///
/// 不能更小: trapframe 272 字节 + 陷阱处理叠几层调用; 不能更大:
/// 64 个进程就是 1 MiB。
pub const KSTACK_SIZE: usize = 4 * crate::mm::pmem::PAGE_SIZE;

/// 进程控制块 (PCB)。
///
/// 内核栈用 `pmem` 单独分配, 因为 64 个 16KiB 数组会让进程表占 1 MiB
/// `.bss`, 而内核栈还需要页对齐 (guard page 按页设权限)。
#[repr(C)]
pub struct Proc {
    /// 进程号 (= 在进程表里的下标)。
    pub pid: usize,
    /// 当前状态。
    pub state: ProcState,
    /// 被换出时保存的上下文。
    pub context: super::context::Context,
    /// 指向本进程内核栈上的 trapframe。
    ///
    /// 必须与汇编共享: 陷阱入口换栈后用固定偏移从 sp 算出 trapframe
    /// 的位置 (栈顶 - TRAPFRAME_SIZE), 所以放在内核栈顶固定位置。
    pub trapframe: *mut oslab_hal::arch::TrapFrame,
    /// 内核栈的栈顶 (高地址)。
    pub kstack_top: usize,
    /// 退出码 (仅对 Zombie 有意义)。
    pub exit_code: usize,
    /// 父进程的 pid (0 表示没有父进程)。
    ///
    /// `wait` 用它: 只有父进程有权回收僵尸子进程。
    pub parent: usize,
    /// 本进程的文件描述符表。
    ///
    /// fd 号是进程私有的命名空间: A 的 fd 3 与 B 的 fd 3 可指向不同
    /// 文件。全局化会让 fork 后子进程关 fd 连带关掉父进程的。
    /// 本进程的页表根页号 (0 表示还没有地址空间)。
    ///
    /// 每进程一份, 让两个进程能在同一虚拟地址放各自代码 (fork 的前提)。
    /// 内核映射在创建页表时已整份复制进去, 陷入内核后代码/栈/设备仍在。
    pub pgtbl: usize,
}

// SAFETY: `Proc` 会被多个 hart 共享。裸指针 `trapframe` 指向本进程
// 自己的内核栈, 同一时刻只有一个 hart 能运行它 (调度器保证), 所以
// 不存在数据竞争。 `Send`/`Sync` 理由相同。
unsafe impl Send for Proc {}
unsafe impl Sync for Proc {}

impl Proc {
    /// 一个空进程槽。
    const fn empty() -> Self {
        Self {
            pid: 0,
            state: ProcState::Unused,
            context: super::context::Context::zeroed(),
            trapframe: core::ptr::null_mut(),
            kstack_top: 0,
            exit_code: 0,
            parent: 0,
            pgtbl: 0,
        }
    }
}

/// 进程表。用静态数组而非堆分配 (理由见 [`NPROC`])。
static mut PROCS: [Proc; NPROC] = [const { Proc::empty() }; NPROC];

// 下一个要分配的 pid (单调递增不回收, 避免旧 pid 突然指向新进程)。
static NEXT_PID: AtomicUsize = AtomicUsize::new(1);

// 当前在跑的进程号, 每个 hart 一份。
static mut CURRENT: [usize; 8] = [0; 8];

/// 初始化进程表, 由启动核调用一次。
pub fn proc_init() {
    // SAFETY: 启动阶段单核, 且这是第一次访问进程表。
    unsafe {
        for i in 0..NPROC {
            PROCS[i] = Proc::empty();
            PROCS[i].pid = i;
        }
    }
}

/// 取当前 hart 正在运行的进程。
pub fn current() -> Option<&'static mut Proc> {
    let cpu = arch::cpu::cpu_id()?;
    // SAFETY: `CURRENT` 每个 hart 一槽, 当前 hart 只有一个执行流。
    let pid = unsafe { CURRENT[cpu] };
    if pid == 0 || pid >= NPROC {
        return None;
    }
    // SAFETY: pid 在界内 (上面检查过), PROCS 是静态数组。
    unsafe { Some(&mut PROCS[pid]) }
}

/// 设置当前 hart 正在运行的进程。
///
/// # Safety
/// 调用者保证 `pid` 在界内, 且不再用先前那个进程的引用 (换走后它
/// 可能被另一个 hart 运行)。
pub unsafe fn set_current(pid: usize) {
    if let Some(cpu) = arch::cpu::cpu_id() {
        // SAFETY: 由调用者保证 pid 合法; cpu 必在界内。
        unsafe {
            CURRENT[cpu] = pid;
        }
    }
}

/// 分配一个空闲进程槽并初始化它的内核栈。返回 `None` 表示表满。
///
/// 建立的不变量: `kstack_top` 页对齐且 `trapframe` 落在栈顶之下
/// `TRAPFRAME_SIZE` 处 (汇编依赖); 状态为 `Unused` 由调用者改;
/// `context` 为空由调用者覆盖。
pub fn proc_alloc() -> Option<&'static mut Proc> {
    for i in 1..NPROC {
        // SAFETY: 单线程访问进程表 (调度器持锁调用本函数)。
        let p = unsafe { &mut PROCS[i] };
        if p.state == ProcState::Unused {
            // 内核栈要 N 个连续页 (栈 sp 从高往低, 中间不能有空洞)。
            let mut base = 0usize;
            for k in 0..(KSTACK_SIZE / pmem::PAGE_SIZE) {
                let page = pmem::pmem_alloc(pmem::Pool::Kernel);
                if page == 0 {
                    // 失败则回滚已分配的页, 不留半个栈 (否则栈增长到
                    // 缺口会踩到别人的内存)。
                    for j in 0..k {
                        // SAFETY: 这些页确实是本次分配且尚未使用。
                        unsafe {
                            pmem::pmem_free(base + j * pmem::PAGE_SIZE, pmem::Pool::Kernel);
                        }
                    }
                    return None;
                }
                if k == 0 {
                    base = page;
                }
            }

            p.kstack_top = base + KSTACK_SIZE;
            // trapframe 放在内核栈顶之下, 汇编从这里取它。
            p.trapframe =
                (p.kstack_top - oslab_hal::arch::TRAPFRAME_SIZE) as *mut oslab_hal::arch::TrapFrame;
            p.state = ProcState::Unused;
            p.exit_code = 0;
            p.parent = 0;
            p.pgtbl = 0;
            p.pid = i;
            NEXT_PID.fetch_add(1, Ordering::Relaxed);
            return Some(p);
        }
    }
    None
}

/// 按 pid 取进程 (不检查状态)。给调度器在**不切换**时查看任意进程
/// 状态用。返回 `None` 表示 pid 越界。
pub fn proc_at(pid: usize) -> Option<&'static mut Proc> {
    if pid == 0 || pid >= NPROC {
        return None;
    }
    // SAFETY: pid 在界内 (上面检查过), PROCS 是静态数组。返回 &mut
    // 的前提是调用者不与别的 hart 同时改同一槽 —— 调度器用"一个进程
    // 同时只在一个 hart 上 Running"这条不变量保证 (见 sched.rs)。
    unsafe { Some(&mut PROCS[pid]) }
}

// `wait` 用来保护"子进程状态 + 睡眠"的锁。"检查有没有僵尸子进程"与
// "入睡"必须在同一把锁下, 否则窗口期子进程退出并 wakeup 后父进程
// 才睡下 -> 永远醒不来 (lost wakeup)。
static WAIT_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();

/// 取 `wait` 用的锁 (供 syscall 层使用)。
pub fn wait_lock() -> &'static crate::sync::SpinLock {
    &WAIT_LOCK
}

/// 复制当前进程 (fork)。
///
/// 四件事: 新页表 + 逐页拷贝用户内存; 复制 trapframe (子进程 a0=0,
/// sepc 跳过 ecall —— fork 调用一次返回两次且值不同); 继承 fd 表;
/// 伪造第一次被调度的上下文 (`ra = forkret`)。
///
/// # Safety
/// 必须在当前进程的上下文里调用。
pub unsafe fn proc_copy(parent_pid: usize) -> Option<usize> { None }

/// 当前 hart 正在运行的进程号 (0 表示没有)。
pub fn current_pid() -> Option<usize> {
    let cpu = arch::cpu::cpu_id()?;
    // SAFETY: 每个 hart 只读自己那一格。
    let pid = unsafe { CURRENT[cpu] };
    if pid == 0 || pid >= NPROC {
        None
    } else {
        Some(pid)
    }
}

/// 让出 CPU, 让调度器挑另一个进程运行。
pub fn sched_yield() {
    crate::sched::pick_next_and_switch();
}

/// 每个 hart 的调度器初始化 (建立 idle 进程)。
pub fn sched_init_hart() {
    crate::sched::init_hart();
}

// TODO(lab-6): 实现 sleep / wakeup, 并想清楚 lost wakeup。
//   检查条件与进入睡眠之间如果中断能进来, 唤醒就会丢失。
//   症状是「系统偶尔卡住」, 无法复现 —— 所以正确的做法是
//   让「持锁进入睡眠」成为一条可检查的不变量, 而不是靠
//   调用者记得。
// ===========================================================================
// 睡眠与唤醒
// ===========================================================================

/// 让当前进程睡眠在 `chan` 上, 直到有人 [`wakeup`] 同一个 `chan`。
///
/// ## 丢失唤醒 (lost wakeup)
///
/// 一个看似正确的错误写法是先放锁再睡眠:
///
/// ```ignore
/// while !condition() { unlock(lk); sleep(chan); lock(lk); }
/// ```
///
/// 在 `unlock` 与 `sleep` 之间的窗口, 唤醒者可能拿到锁、置条件、
/// `wakeup` —— 此刻目标还没睡, 唤醒就丢了, 该进程永远不醒。
///
/// 正确做法是睡眠者在**持锁**下把自己标记为 Sleeping 并入队。唤醒者
/// 要么在入睡前拿到锁 (看到条件满足, 睡眠者不再睡), 要么在入睡后
/// 拿到锁 (看到 Sleeping 并唤醒它), 两种顺序都不丢。本函数顺序:
/// 持锁 -> 标 Sleeping (关键, 持锁时做) -> 记 chan -> 放锁 ->
/// 切到调度器 -> 醒来后重新取锁。
///
/// # Safety
/// 调用者必须持有保护睡眠条件的锁, 且 `lk` 就是那把锁 —— 本函数在
/// 入睡前释放它、醒来后重新获取。
pub unsafe fn sleep(chan: usize, lk: &crate::sync::SpinLock) { }

/// 唤醒所有睡眠在 `chan` 上的进程。
///
/// # Safety
/// 调用者应持有与睡眠者相同的锁, 让"条件已满足"与"进程被标为
/// Runnable"对睡眠者表现为一个原子事件 (见 [`sleep`])。
pub unsafe fn wakeup(chan: usize) { }

// 每个进程等待的事件标识 (0 表示不等待任何东西)。
static SLEEP_CHAN: [AtomicUsize; NPROC] = [const { AtomicUsize::new(0) }; NPROC];