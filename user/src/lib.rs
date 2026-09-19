//! `oslab_user` —— 用户态运行时 (独立 crate)。
//!
//! 用户程序 (`src/bin/*.rs`) 通过 `use oslab_user::*;` 使用本运行时:
//! 系统调用包装 (`write` / `getpid` / `fork` / ...)、打印 (`println!` /
//! `print!`)、入口包装 (`entry!`)、panic 处理。本文件只是把
//! [`runtime`] 模块的内容再导出到 crate 根, 让调用路径最自然。

#![no_std]

// 架构层 (系统调用机制): 由 cargo feature `arch-*` 选择, 见 arch/mod.rs。
mod arch;

mod runtime;

pub use runtime::*;
