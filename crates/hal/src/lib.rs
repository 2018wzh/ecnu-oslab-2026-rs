//! oslab-hal — 硬件抽象层: arch (CPU/ISA 语义) + platform (机器常量)。
//!
//! arch 用 `#[cfg]` 在模块级选择唯一架构, 对上层暴露一批同名同签名
//! 的自由函数/常量 (`arch::irq::disable()`), 不用 trait/dyn; platform
//! 是编译期选定的静态常量。
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

pub mod arch;
pub mod platform;
pub mod putchar;

/// 编译期断言工具: 失败时以可读的 `MSG` 作为编译错误信息。
pub mod static_assert {
    /// 检查条件 `COND`, 失败时报 `MSG`。原理: 用 const 数组长度做断言,
    /// 条件为假时长度的值为 -1, 编译报错并打印 `MSG`。
    #[macro_export]
    macro_rules! static_assert {
        ($cond:expr, $msg:literal $(,)?) => {
            const _: [(); 0] = [(); ($cond) as usize - 1];
            const _: &str = $msg;
        };
    }
}
