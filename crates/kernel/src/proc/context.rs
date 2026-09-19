//! 上下文切换保存的寄存器集合。
//!
//! 与 `TrapFrame` 的区别 (本 lab 最重要的概念):
//!   TrapFrame  "被中断那一刻 CPU 的全部现场" (31 个通用寄存器 + sepc +
//!             sstatus) —— 属于 ABI, 用户程序可能用到任何一个寄存器。
//!   Context    "编译器认为哪些变量在函数调用后还要活着" (只有
//!             callee-saved) —— 属于调用约定, 其余寄存器本就被调用者破坏。
//! 混淆两者的后果很难查: 多保存只浪费几个 cycle, 少保存会让某个
//! callee-saved 寄存器在两个进程间泄漏 (症状"进程 A 的局部变量偶尔
//! 变成进程 B 的值", 定位以小时计)。
//!
//! RISC-V callee-saved: s0..s11 (12 个) + sp (换进程必须换) + ra (切换
//! 语境下必须保存, 恢复后要回到离开的位置)。`tp` 不在内: 它存 hartid,
//! 属于 CPU 而非进程, 保存它会让进程被调度到别的核时读到错误 hartid。

/// 一个进程被换出时保存的寄存器集合。
///
/// 字段顺序必须与 `switch.rs` 汇编严格一致 (汇编用固定偏移访问), 下面
/// 的 `const _` 断言把每个偏移固定下来, 有人插字段会编译失败而非运行期
/// 出错。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    /// 返回地址 (x1)。切换回来时从这里继续执行。
    pub ra: usize,
    /// 栈指针 (x2)。每个进程有自己的内核栈。
    pub sp: usize,
    /// s0 (x8) —— callee-saved。
    pub s0: usize,
    /// s1 (x9) —— callee-saved。
    pub s1: usize,
    /// s2 —— callee-saved。
    pub s2: usize,
    /// s3 —— callee-saved。
    pub s3: usize,
    /// s4 —— callee-saved。
    pub s4: usize,
    /// s5 —— callee-saved。
    pub s5: usize,
    /// s6 —— callee-saved。
    pub s6: usize,
    /// s7 —— callee-saved。
    pub s7: usize,
    /// s8 —— callee-saved。
    pub s8: usize,
    /// s9 —— callee-saved。
    pub s9: usize,
    /// s10 —— callee-saved。
    pub s10: usize,
    /// s11 —— callee-saved。
    pub s11: usize,
}

// ---------------------------------------------------------------------------
// 编译期断言: 字段偏移固定 —— 让后来的人改不坏 (插字段会立刻编译失败)。
// ---------------------------------------------------------------------------
const _: () = {
    assert!(core::mem::offset_of!(Context, ra) == 0 * 8);
    assert!(core::mem::offset_of!(Context, sp) == 1 * 8);
    assert!(core::mem::offset_of!(Context, s0) == 2 * 8);
    assert!(core::mem::offset_of!(Context, s1) == 3 * 8);
    assert!(core::mem::offset_of!(Context, s2) == 4 * 8);
    assert!(core::mem::offset_of!(Context, s11) == 13 * 8);
    assert!(core::mem::size_of::<Context>() == 14 * 8);
};

impl Context {
    /// 一个全零的上下文 (进程尚未运行过)。
    pub const fn zeroed() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s0: 0,
            s1: 0,
            s2: 0,
            s3: 0,
            s4: 0,
            s5: 0,
            s6: 0,
            s7: 0,
            s8: 0,
            s9: 0,
            s10: 0,
            s11: 0,
        }
    }

    /// 为"第一次被调度"准备一个上下文。
    ///
    /// 新进程从没在 CPU 上跑过, 没有可保存的现场, 但 `context_switch`
    /// 无条件"保存当前 -> 恢复目标", 所以必须伪造一个"刚好被换出过"
    /// 的现场: ra=entry (切进来后从这返回), sp=自己的内核栈。
    /// **创建进程就是伪造一个上下文。**
    pub const fn new(entry: usize, kernel_stack_top: usize) -> Self {
        let mut c = Self::zeroed();
        c.ra = entry;
        c.sp = kernel_stack_top;
        c
    }
}