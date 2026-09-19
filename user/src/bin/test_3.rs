//! # `test_3` —— fork / wait / 退出状态
//!
//! 验证: fork 后父子各得不同返回值; 子进程退出状态原样传回父进程 (wait
//! 唯一有价值的输出); wait 真的在等。退出状态选了 7 —— 非 0 非 -1,
//! 日志里一眼可认出是子进程传回的, 而非默认值。

#![no_std]
#![no_main]

// 把 `oslab-user` 运行时当作普通 crate 依赖使用 (`use` 即可),
// 程序由 cargo 链接 (见 user/build.rs 与 xtask/src/user.rs)。
// 运行时是接口, 每个程序只用其中一部分 —— 未用部分不该各报一遍
// dead_code, 故在 crate 根允许。
#![allow(dead_code)]

use oslab_user::*;


fn main() {
    println!("---- test_3: fork / wait / exit ----");

    let pid = match oslab_user::fork() {
        Ok(p) => p,
        Err(_) => {
            println!("[test_3] FAIL: fork 失败");
            oslab_user::exit(1);
        }
    };

    if pid == 0 {
        let me = oslab_user::getpid().unwrap_or(0);
        println!("[test_3] 子进程 pid={}, 即将以状态 7 退出", me);
        oslab_user::exit(7);
    }

    println!("[test_3] 父进程 fork 返回子 pid={}", pid);

    let mut status: usize = 0;
    let waited = match oslab_user::wait_status(&mut status) {
        Ok(w) => w,
        Err(_) => {
            println!("[test_3] FAIL: wait 失败");
            oslab_user::exit(1);
        }
    };
    println!(
        "[test_3] wait 返回 {}, 子进程状态 = {} (期望 7)",
        waited, status
    );

    if status != 7 {
        oslab_user::exit(1);
    }
    println!("[test_3] OK");
    oslab_user::exit(0);
}

entry!(main);
