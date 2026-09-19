//! # `hello` —— 最小用户程序
//!
//! 只有两行输出, 用于隔离问题: init 不输出但 hello 输出 => 问题在 init
//! 逻辑里; 两者都不输出 => 问题在内核装载/进入用户态。

#![no_std]
#![no_main]

// 把 `oslab-user` 运行时当作普通 crate 依赖使用 (`use` 即可),
// 程序由 cargo 链接 (见 user/build.rs 与 xtask/src/user.rs)。
// 运行时是接口, 每个程序只用其中一部分 —— 未用部分不该各报一遍
// dead_code, 故在 crate 根允许。
#![allow(dead_code)]

use oslab_user::*;


fn main() {
    println!("[hello] 我是用 Rust 编译的用户程序。");
    println!("[hello] 能看到这一行, 说明用户态是通的。");
}

entry!(main);
