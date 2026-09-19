//! # `test_1` —— 基本输出与系统调用返回值
//!
//! 验证用户态 `write` 到达内核且返回值正确。用 `s.len()` 而非手写数字:
//! 长度错误会被截断且像内核 write 坏了, Rust 切片自带长度, 从源头避免。

#![no_std]
#![no_main]

// 把 `oslab-user` 运行时当作普通 crate 依赖使用 (`use` 即可),
// 程序由 cargo 链接 (见 user/build.rs 与 xtask/src/user.rs)。
// 运行时是接口, 每个程序只用其中一部分 —— 未用部分不该各报一遍
// dead_code, 故在 crate 根允许。
#![allow(dead_code)]

use oslab_user::*;

fn main() {
    println!("---- test_1: 基本输出 ----");

    let to_out: &[u8] = "[test_1] 这一行走 stdout\n".as_bytes();
    match oslab_user::write(oslab_user::STDOUT_FILENO, to_out) {
        Ok(n) if n == to_out.len() => {
            println!("[test_1] write 返回 {} (正确)", n);
        }
        Ok(n) => {
            println!("[test_1] FAIL: write 返回 {}, 期望 {}", n, to_out.len());
            oslab_user::exit(1);
        }
        Err(_) => {
            println!("[test_1] FAIL: write 失败");
            oslab_user::exit(1);
        }
    }

    // stderr 是另一个 fd: 内核要分别处理 (同一个控制台, 不同的 file 对象)。
    let _ = oslab_user::write(oslab_user::STDERR_FILENO, "[test_1] 这一行走 stderr\n".as_bytes());

    match oslab_user::getpid() {
        Ok(pid) => println!("[test_1] getpid() = {}", pid),
        Err(_) => {
            println!("[test_1] FAIL: getpid 失败");
            oslab_user::exit(1);
        }
    }

    println!("[test_1] OK");
    oslab_user::exit(0);
}

entry!(main);
