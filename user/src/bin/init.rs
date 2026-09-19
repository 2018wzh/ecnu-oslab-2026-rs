//! # `init` —— 第一个 Rust 用户程序
//!
//! 依次验证用户态通路: 装载并 `sret` 进入, `write`, `getpid`, `exit`。
//! 任何一步坏了都是"什么都没有输出", 故每步单独打印一行以便二分定位。

#![no_std]
#![no_main]

// 把 `oslab-user` 运行时当作普通 crate 依赖使用 (`use` 即可),
// 程序由 cargo 链接 (见 user/build.rs 与 xtask/src/user.rs)。
// 运行时是接口, 每个程序只用其中一部分 —— 未用部分不该各报一遍
// dead_code, 故在 crate 根允许。
#![allow(dead_code)]

use oslab_user::*;

fn main() {
    // lab-4..8 阶段: 用户进程只发一个最简单的系统调用 SYS_HELLOWORLD,
    // 内核打印固定字符串。write / fd 表 / 文件抽象属于 lab-9。
    let _ = oslab_user::helloworld();
    let _ = oslab_user::helloworld();

    // ---- 验证 getpid ----
    // 这一步很关键: pid 是内核才知道的信息。用户程序能读到它,
    // 说明"陷入内核 -> 读 trapframe -> 写回 a0 -> sret 返回"
    // 整条通路都是正确的, 而不只是"恰好打印出了常量字符串"。
    loop {
        core::hint::spin_loop();
    }
}

entry!(main);
