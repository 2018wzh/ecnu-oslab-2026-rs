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
    let _ = oslab_user::helloworld();
    let _ = oslab_user::helloworld();
    loop { core::hint::spin_loop(); }
}

entry!(main);
