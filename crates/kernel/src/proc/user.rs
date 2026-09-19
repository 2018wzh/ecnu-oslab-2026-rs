//! 进入用户态。
//!
//! 没有一条"进入用户态"的指令: 用户态靠 **sret** (从陷阱返回) 进去
//! (sret 读"返回目标特权级"状态位, 若为 U-mode 就切到用户态并跳到
//! trapframe 记录的地址)。所以"启动一个用户程序" = 造一个 trapframe
//! 填成"好像刚从用户态陷入"的样子 + sstatus 目标特权级设 U-mode +
//! 调 `trap_return`。
//!
//! 最容易错的地方: 目标特权级必须显式设置 (漏掉会让 CPU 以 S-mode
//! 执行用户地址代码, 表现为"第一个系统调用后什么都输出不了, 静默卡死"),
//! 由 hal 的 `build_user_return_status` 做成一个函数而非"记得清那一位"。

use oslab_hal::arch::{TrapFrame, TRAPFRAME_SIZE};

use crate::mm::pmem;
use crate::mm::uvm::{USER_BASE, USER_STACK_BASE, USER_STACK_SIZE};
use crate::proc;

/// 新进程**第一次被调度**时的内核入口。
///
/// 不是被 `call` 进来的, 而是被恢复上下文 (`context_switch` 的 `ret`)
/// 进入的: 一开始执行用的就是子进程自己的内核栈。只做一件事 —— 和
/// [`enter_user`] 最后一步相同, 把 CPU 交还给 trapframe 描述的用户态。
///
/// # Safety
/// 必须是在当前进程的上下文里第一次被调度 (trapframe 已准备好)。
pub unsafe extern "C" fn forkret() -> ! { unimplemented!() }

/// 让当前进程"返回"到用户态。
///
/// # Safety
/// 当前进程必须已由 [`proc_make_user`] 准备好 trapframe, 用户页已映射。
/// 本函数不返回。
pub unsafe fn enter_user() -> ! { unimplemented!() }

/// 从 **ELF 映像**创建一个用户进程 (从磁盘装入时走这条路)。
///
/// 与 [`proc_make_user`] 的区别只有代码段怎么来: 前者把整段扁平字节
/// 拷到 USER_BASE 且入口就是 USER_BASE; 本函数按 ELF 的 program header
/// 把每个段装到它声明的地址, 用 e_entry 作入口 —— 这才是真实 OS 的
/// 做法, 内核不依赖任何约定。
pub fn proc_make_user_elf(pid: usize, image: &[u8]) -> Result<u64, super::elf::ElfError> {
    let Some(p) = proc::proc_at(pid) else {
        return Err(super::elf::ElfError::LoadFailed);
    };
    // ---- 0. 为本进程建一张私有页表 ----
    // 不能共用内核全局页表, 否则两个进程的用户页面落在同一虚拟地址
    // 互相覆盖。新表已复制内核全部映射, 陷入内核后一切照旧。
    let Some(root) = crate::mm::vm::kvm_create_process_table() else {
        return Err(super::elf::ElfError::LoadFailed);
    };
    let pt = &mut crate::mm::vm::PageTable::from_root(root);
    p.pgtbl = root.0 << 12;   /* 记下根页的物理地址 (调 satp 用) */

    // ---- 1. 按 ELF 的 program header 装载各个段 ----
    let entry = super::elf::load_into(pt, image)?;
    super::elf::check_entry(entry)?;

    // ---- 1.5 建立标准输入/输出/错误 ----
    // 用户程序第一行输出是 write(1,...), 会先查 fd 表; 缺了返回 NoFd,
    // 现象是"跑完了但看不到输出"。放这里保证任何创建用户进程的路径
    // 都建立了 fd 0/1/2。
    p.fds.open_stdio();

    // ---- 2. 分配并映射用户栈 ----
    // ELF 只描述代码与数据, 栈由内核分配, 放在独立高地址 USER_STACK_BASE。
    let stack = pmem::pmem_alloc(pmem::Pool::User);
    if stack == 0 {
        return Err(super::elf::ElfError::LoadFailed);
    }
    let stack_ppn = oslab_hal::arch::mm::PhysAddr(stack).page_num();
    let stack_perms = oslab_hal::arch::mm::Perms::READ
        .or(oslab_hal::arch::mm::Perms::WRITE)
        .or(oslab_hal::arch::mm::Perms::USER);
    // SAFETY: pt 是内核页表; USER_STACK_BASE 页对齐; stack 是刚分配的页。
    if unsafe { crate::mm::vm::map(pt, USER_STACK_BASE, stack_ppn, stack_perms) }.is_err() {
        return Err(super::elf::ElfError::LoadFailed);
    }

    // ---- 3. 造 trapframe ----
    // SAFETY: p.trapframe 指向本进程内核栈顶之下的一段保留区域。
    let tf: &mut TrapFrame = unsafe { &mut *p.trapframe };
    tf.set_faulting_pc(entry as usize);
    tf.set_user_sp(USER_STACK_BASE + USER_STACK_SIZE);
    tf.set_kernel_stack_top(p.kstack_top);
    tf.set_a0(0);
    tf.set_saved_status(0);

    p.state = proc::ProcState::Runnable;
    Ok(entry)
}

/// 用一段 ELF 映像**替换当前进程**的地址空间 (exec 系统调用核心)。
///
/// 与 [`proc_make_user_elf`] 的区别: 前者服务新进程 (启动时创建 pid 1),
/// 本函数服务已在跑的进程, 不碰 pid/父子关系/fd 表, 只换"用户地址空间
/// + 寄存器现场" (fd 表保留: exec 是"同一进程换程序", shell 先 fork 再
/// exec, 命令期待 stdout 是继承下来的)。
///
/// 顺序必须"新地址空间全部造好 -> 才回收旧的 -> 才切换": 装载可能失败,
/// 而 exec 失败语义是"返回错误码继续跑原程序", 先释放旧空间则失败时
/// 就没映像可跑了。
pub fn exec_current(image: &[u8]) -> Result<u64, super::elf::ElfError> { unimplemented!() }

/// 编译期断言: trapframe 放得下。
const _: () = {
    assert!(TRAPFRAME_SIZE <= proc::KSTACK_SIZE);
};