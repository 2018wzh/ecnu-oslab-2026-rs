//! # `test_2` —— 从磁盘读一个文件
//!
//! 本版文件系统只读: `open` 不接受 `CREATE`, `write` 只支持控制台与设备。
//! 验证块设备 → fs → inode → fd 表 → 用户缓冲区链路通, 且 `lseek` 能
//! 把读位置移回去。

#![no_std]
#![no_main]

// 把 `oslab-user` 运行时当作普通 crate 依赖使用 (`use` 即可),
// 程序由 cargo 链接 (见 user/build.rs 与 xtask/src/user.rs)。
// 运行时是接口, 每个程序只用其中一部分 —— 未用部分不该各报一遍
// dead_code, 故在 crate 根允许。
#![allow(dead_code)]

use oslab_user::*;


fn main() {
    println!("---- test_2: 读文件 ----");

    // 打开磁盘上的 /hello（它就在同一张盘上, 由 xtask disk 写入）。
    let fd = match oslab_user::open(b"/hello\0", oslab_user::open_mode::RDONLY) {
        Ok(fd) => fd,
        Err(_) => {
            println!("[test_2] FAIL: open /hello 失败");
            oslab_user::exit(1);
        }
    };
    println!("[test_2] /hello 的 fd = {}", fd);

    let mut buf = [0u8; 64];
    let n = match oslab_user::read(fd, &mut buf) {
        Ok(n) => n,
        Err(_) => {
            println!("[test_2] FAIL: read 失败");
            oslab_user::exit(1);
        }
    };
    println!("[test_2] 第一次读到 {} 字节", n);

    // ---- lseek 回到开头, 再读一次 ----
    // 不 lseek 偏移停在末尾, 第二次 read 返回 0, 易被误判成"文件是空的"。
    if oslab_user::lseek(fd, 0).is_err() {
        println!("[test_2] FAIL: lseek 失败");
        oslab_user::exit(1);
    }
    let mut buf2 = [0u8; 64];
    let n2 = match oslab_user::read(fd, &mut buf2) {
        Ok(n) => n,
        Err(_) => {
            println!("[test_2] FAIL: 第二次 read 失败");
            oslab_user::exit(1);
        }
    };
    if n2 != n {
        println!("[test_2] FAIL: lseek 之后读到 {} 字节, 第一次是 {}", n2, n);
        oslab_user::exit(1);
    }
    println!("[test_2] lseek 回到开头, 再读仍是 {} 字节 (一致)", n2);

    let _ = oslab_user::close(fd);
    println!("[test_2] OK");
    oslab_user::exit(0);
}

entry!(main);
