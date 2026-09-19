//! # `test_4` —— exec 替换自身
//!
//! exec 成功之后不返回 (进程映像被整个换掉)。exec 之后的打印永远不该
//! exec 之后的打印永远不该出现 —— 出现就说明内核"加载新程序还回到老程序"。

#![no_std]
#![no_main]

// 把 `oslab-user` 运行时当作普通 crate 依赖使用 (`use` 即可),
// 程序由 cargo 链接 (见 user/build.rs 与 xtask/src/user.rs)。
// 运行时是接口, 每个程序只用其中一部分 —— 未用部分不该各报一遍
// dead_code, 故在 crate 根允许。
#![allow(dead_code)]

use oslab_user::*;


fn main() {
    println!("---- test_4: exec 替换自身 ----");
    let me = oslab_user::getpid().unwrap_or(0);
    println!("[test_4] 我是 pid={}, 现在 exec /hello", me);

    // 成功的话永远到不了这里。
    match oslab_user::exec(b"/hello\0") {
        Ok(_) => {
            println!("[test_4] FAIL: exec 返回了");
            oslab_user::exit(1);
        }
        Err(_) => {
            println!("[test_4] FAIL: exec 失败");
            oslab_user::exit(1);
        }
    }
}

entry!(main);
