//! 设备协议驱动: 实现各设备的寄存器协议 (PLIC、16550、virtio、SDHCI)。
//!
//! 设备的位置与参数 (基地址、中断号、时钟) 来自 `oslab_hal::platform`,
//! 这里不出现任何具体地址。驱动通过持有的 `&Platform` 取参数。
//!
//! 块设备运行期可能有多个, 用 [`block::BlockDevice`] trait 对象;
//! UART/PLIC 每台机器只有一个, 用具体类型。
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

// ---- 各子系统模块 ----
pub mod mmio;
pub mod serial;
pub mod irqchip;
// ---- 本阶段模块列表结束 ----
