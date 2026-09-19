//! 上下文切换的汇编实现。
//!
//! `context_switch(old, new)`: 把当前 callee-saved 存进 *old, 把 *new
//! 装回 CPU, 然后 `ret` —— 于是执行流跑进**另一个函数**。ret 不回到
//! 调用 context_switch 的函数, 而是回到 new->ra (可能是某进程上次换出
//! 的位置, 也可能是从未运行过进程的入口, 见 `Context::new`)。
//!
//! 用 `global_asm!` 而非 `#[naked]`/内联汇编: 需要精确控制每条指令对
//! sp 的改动, 编译器插入的序言/尾声会写在已换的栈上。偏移量用
//! `offset_of!` 编译期算出而非手写数字, 有人改 Context 不会与汇编脱节。

use core::mem::offset_of;

use super::context::Context;

core::arch::global_asm!(
    r#"
.section .text
.align 4
.global context_switch
context_switch:
    j       context_switch_unimplemented
"#
);

#[unsafe(no_mangle)]
extern "C" fn context_switch_unimplemented() -> ! {
    panic!("上下文切换还没实现 (见本分支 README 的\"需要你完成的部分\")")
}

unsafe extern "C" {
    /// 保存 `old` 的现场, 恢复 `new` 的现场。
    ///
    /// # Safety
    /// * `old` 与 `new` 必须指向有效且互不重叠的 `Context`;
    /// * `new.sp` 必须指向该进程自己的、已映射的内核栈;
    /// * `new.ra` 必须是合法代码地址 —— 若是从未运行过进程的入口,
    ///   该入口必须不返回。
    ///
    /// 签名是 `()` 而非 `!`: 它确实会返回 (当有人切回来时), 只是
    /// 不是这一次调用。
    pub fn context_switch(old: *mut Context, new: *const Context);
}

/// 保存当前现场到 `old`, 然后切到 `new` (上层的安全包装)。
///
/// # Safety
/// * `old` 指向的内存在本进程换回来之前不会被复用;
/// * `new` 描述一个"可以开始执行"的上下文;
/// * **返回后全局状态可能已属于另一个进程**, 不要在调用前后假设
///   任何锁仍被自己持有。
pub unsafe fn switch(old: &mut Context, new: &Context) {
    // SAFETY: 由调用者保证 (见上面的 Safety 段)。
    unsafe {
        context_switch(old as *mut Context, new as *const Context);
    }
}