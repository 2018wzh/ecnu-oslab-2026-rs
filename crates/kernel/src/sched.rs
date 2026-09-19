//! 轮转调度器。
//!
//! 为什么必须有 idle 进程: 每个 hart 需要一个合法执行流 (trapframe 落在
//! 当前进程的内核栈上; 从核退出调度器后也要有处可去)。所以给每个 hart
//! 建一个永远 Runnable、只有没别人可选时才被选中的 idle 进程。
//!
//! 轮转没有可调参数, "调度器有 bug"与"调度策略不合适"不会混在一起。
//! 不变量: 同一时刻一个进程只能在一个 hart 上 Running; 持锁睡眠期间
//! 锁必须已释放; idle 永远可被选中, 因此 pick_next 永不返回 None。

use core::sync::atomic::{AtomicUsize, Ordering};

use oslab_hal::arch;

use crate::proc::{self, ProcState};

// 记住每个 hart 上次选中的下标, 从它之后开始找 (轮转的最小改动)。
static mut LAST_PICKED: [usize; 8] = [0; 8];

// 每个 hart 的 idle 进程号 (0 表示还没有)。
static mut IDLE_PROC: [usize; 8] = [0; 8];

// 调度器是否已初始化。
static INITED: AtomicUsize = AtomicUsize::new(0);

/// idle 进程的内核入口。
///
/// `context_switch` 恢复 `Context.ra`, 新进程第一次被调度时从 `ra` 开始
/// 执行, 所以每个进程都必须有合法入口。本函数永不返回: 没有别的进程
/// 可跑时占着 CPU 等中断。
extern "C" fn idle_main() -> ! { unimplemented!() }

/// 为一个 hart 建立 idle 进程 (每个 hart 各调一次)。
pub fn init_hart() {
    let Some(cpu) = arch::cpu::cpu_id() else {
        return;
    };

    // 每个 hart 一个 idle, 不能共用: idle 也有内核栈放 trapframe,
    // 共用会让两个 hart 写到同一栈上 (随机内存破坏)。
    let Some(p) = proc::proc_alloc() else {
        oslab_hal::putchar::puts("[oslab-rs] FATAL: 无法分配 idle 进程\n");
        arch::time::park_current_hart();
    };
    p.state = ProcState::Runnable;
    // 伪造"第一次被调度"的现场: ra=入口, sp=自己的内核栈顶 ——
    // 这就是"创建进程"的全部秘密 (见 Context::new)。
    p.context = crate::proc::context::Context::new(idle_main as *const () as usize, p.kstack_top);

    // SAFETY: 每个 hart 只写自己那一格。
    unsafe {
        IDLE_PROC[cpu] = p.pid;
    }
    INITED.store(1, Ordering::Release);
}

/// 挑一个可运行的进程并切换过去。
///
/// 对调用者而言它"不返回": 返回后世界已变 (可能换了进程, 或换回自己
/// 但隔了许久), 不能假设任何调用前的状态还在。
pub fn pick_next_and_switch() { }

/// 切到 `target` 进程。
///
/// 最容易写错的一处: `context_switch` 之后旧 `&mut` 引用不再有效 (那个
/// 进程可能已在别的 hart 上跑)。所以把引用取得尽量紧凑、切后不再碰旧值。
fn switch_to(cpu: usize, target_pid: usize) {
    let cur_pid = proc::current_pid().unwrap_or(0);

    // 自己切给自己: 什么也不做。
    if cur_pid == target_pid {
        return;
    }

    // 当前进程只是换出, 标回 Runnable; 目标进程标为 Running。
    if let Some(cur) = proc::proc_at(cur_pid) {
        if cur.state == ProcState::Running {
            cur.state = ProcState::Runnable;
        }
    }
    if let Some(t) = proc::proc_at(target_pid) {
        t.state = ProcState::Running;
    }

    // ---- 取两个上下文指针 ----
    // 分两个独立块而非一次取两个 &mut: 它们指向同一 static 的不同槽,
    // 用裸指针明确表达"这是两个不同的对象"。
    let old_ptr = match proc::proc_at(cur_pid) {
        Some(p) => &mut p.context as *mut _,
        None => return,
    };
    let new_ptr = match proc::proc_at(target_pid) {
        Some(p) => &p.context as *const _,
        None => return,
    };

    // ---- 更新"当前进程"必须在切换前做 ----
    // 目标进程从 context_switch 返回点 (或 forkret) 继续执行时, 读
    // current() 必须拿到自己。
    //
    // SAFETY: 每个 hart 只写自己那一格; 此刻还没切走。
    unsafe {
        proc::set_current(target_pid);
    }

    // ---- 切换地址空间必须在 context_switch 之前 ----
    // context_switch 末尾是 ret, 跳到目标的内核代码; 而目标随后 sret
    // 到用户态, 那一刻 satp 必须已是它自己的表。idle 的 pgtbl 为 0,
    // 此时用内核全局页表 —— 让 satp 始终等于"当前进程该用的那张"。
    if let Some(t) = proc::proc_at(target_pid) {
        let root = if t.pgtbl != 0 {
            oslab_hal::arch::mm::PhysAddr(t.pgtbl).page_num()
        } else {
            match crate::mm::vm::kvm_global() {
                Some(pt) => pt.root_ppn(),
                None => return,
            }
        };
        // SAFETY: root 是目标页表根; 其内核部分与当前页表一致, 切换后
        // 这段内核代码依然有效。
        unsafe {
            oslab_hal::arch::mm::activate_page_table(root);
        }
    }

    // SAFETY:
    //   * 两个指针来自进程表的不同下标, 互不重叠;
    //   * 目标 context 描述了可开始执行的状态;
    //   * 切后不再使用 old_ptr。
    unsafe {
        proc::switch::context_switch(old_ptr, new_ptr);
    }

    let _ = cpu;
}

/// 调度器是否已初始化。
pub fn inited() -> bool {
    INITED.load(Ordering::Acquire) != 0
}