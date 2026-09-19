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

use crate::proc;

/// 从 **ELF 映像**创建一个用户进程 (从磁盘装入时走这条路)。
///
/// 与 [`proc_make_user`] 的区别只有代码段怎么来: 前者把整段扁平字节
/// 拷到 USER_BASE 且入口就是 USER_BASE; 本函数按 ELF 的 program header
/// 把每个段装到它声明的地址, 用 e_entry 作入口 —— 这才是真实 OS 的
/// 做法, 内核不依赖任何约定。
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

/// 编译期断言: trapframe 放得下。
const _: () = {
    assert!(TRAPFRAME_SIZE <= proc::KSTACK_SIZE);
};