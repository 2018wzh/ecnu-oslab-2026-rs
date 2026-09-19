//! 中断控制器驱动。目前只有 SiFive PLIC, 一份源码服务 QEMU virt
//! 和 VisionFive2 两台机器 (基地址由 platform 层提供)。

pub mod plic;

pub use plic::{Plic, PlicError};
