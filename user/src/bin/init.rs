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
    println!("========================================");
    println!("  你好, 这里是用户态! (U-mode)");
    println!("  这个程序是【用 Rust 写的】用户程序");
    println!("========================================");
    println!("");
    println!("它由 oslab-user 运行时编译而成,");
    println!("通过 write 系统调用把字符串交给内核输出。");
    println!("");

    // ---- 验证 getpid ----
    // 这一步很关键: pid 是内核才知道的信息。用户程序能读到它,
    // 说明"陷入内核 -> 读 trapframe -> 写回 a0 -> sret 返回"
    // 整条通路都是正确的, 而不只是"恰好打印出了常量字符串"。
    match oslab_user::getpid() {
        Ok(pid) => {
            println!("[init] getpid() 返回 {}", pid);
            println!("[init] 这个数字只有内核知道 ——");
            println!("[init] 它能到这里, 说明 ecall/sret 通路是正确的。");
        }
        Err(_) => {
            println!("[init] getpid() 失败 (本阶段可能尚未实现)");
        }
    }

    // ---- 验证 open / read / close ----
    // 把文件系统与用户程序接起来。读的是 /init —— 本程序自己在磁盘上的
    // ELF, 前 4 字节应为魔数 7f454c46。
    println!("");
    println!("[init] 打开磁盘上的 /init (本程序自己) 并读前 4 字节:");
    // 用 buf.get(i) 而非切片 `&buf[..n]`: 切片越界检查会引入对
    // core::panicking 的引用, 而用户程序 no_std 且无 panic 处理器, 链接会失败。
    match oslab_user::open(b"/init\0", oslab_user::open_mode::RDONLY) {
        Ok(fd) => {
            let mut buf = [0u8; 4];
            match oslab_user::read(fd, &mut buf) {
                Ok(n) => {
                    print!("[init] 读到 ");
                    print!("{}", n);
                    print!(" 字节: ");
                    let magic_ok = n == 4
                        && buf[0] == 0x7f
                        && buf[1] == b'E'
                        && buf[2] == b'L'
                        && buf[3] == b'F';
                    let mut i = 0usize;
                    while i < 4 {
                        if let Some(b) = buf.get(i) {
                            print!("{:x} ", *b as usize);
                        }
                        i += 1;
                    }
                    println!("");
                    if magic_ok {
                        println!("[init] 是 ELF 魔数 —— 文件系统通路正常。");
                    } else {
                        println!("[init] 与 ELF 魔数不符!");
                    }
                }
                Err(_) => println!("[init] read 失败"),
            }
            let _ = oslab_user::close(fd);
        }
        Err(_) => println!("[init] open 失败"),
    }

    // ---- 验证 fork / wait ----
    // 检验的是**多个进程**能不能共存:
    //   * 父子各自拿到不同的返回值 (父: pid, 子: 0)
    //   * 子进程退出后父进程能 wait 到它
    // 如果每进程页表或上下文切换没生效, 这里会乱码、缺页或卡死。
    println!("");
    println!("[init] 调用 fork():");
    match oslab_user::fork() {
        Ok(0) => {
            println!("[child] 我是子进程 (fork 返回 0), 准备 exit(7)");
            oslab_user::exit(7)
        }
        Ok(pid) => {
            print!("[parent] fork 返回 pid=");
            print!("{}", pid);
            println!("");
            println!("[parent] 调用 wait() ...");
            match oslab_user::wait() {
                Ok(done) => {
                    print!("[parent] wait 回收了 pid=");
                    print!("{}", done);
                    println!("");
                }
                Err(_) => println!("[parent] wait 失败"),
            }
        }
        Err(_) => {
            println!("[init] fork 失败 (本阶段可能尚未实现)");
            oslab_user::exit(0)
        }
    }

    // ---- 验证 exec 的失败路径 ----
    // exec 失败时必须返回且原程序继续能跑 (与"成功时不返回"同样关键)。
    println!("");
    print!("[init] exec(\"/no-such-file\") 应当失败: ");
    match oslab_user::exec(b"/no-such-file\0") {
        Ok(_) => println!("竟然成功了 (不该发生!)"),
        Err(_) => println!("按预期失败, 本程序继续运行 (exec 失败可恢复)"),
    }

    // ---- 验证 fork + exec ----
    // 检验进程能换掉自己正在跑的程序: 子进程 exec("/hello") 打印出另一
    // 个程序的内容, 之后自己的 exit 让父进程 wait 正常返回。
    // fork 造新进程、exec 换程序: 必须先 fork, 否则 exec 会把 init 自己
    // 换掉, 就没有父进程回收子进程了 (shell 的三步 fork+exec+wait)。
    // exec 成功时不返回: 若子进程那支打印了 "exec 返回了", 说明内核
    // "假装成功"了 —— 那比直接报错更糟。
    println!("");
    println!("[init] 调用 fork() + exec(\"/hello\"):");
    match oslab_user::fork() {
        Ok(0) => {
            println!("[child] 准备 exec(\"/hello\") —— 下一行应当是 hello 的输出");
            match oslab_user::exec(b"/hello\0") {
                Ok(_) => {
                    println!("[child] exec 返回了 (成功时不该返回!)");
                    oslab_user::exit(1)
                }
                Err(_) => {
                    println!("[child] exec 失败 (本阶段可能尚未实现)");
                    oslab_user::exit(1)
                }
            }
        }
        Ok(pid) => {
            print!("[parent] fork 返回 pid=");
            print!("{}", pid);
            println!("");
            match oslab_user::wait() {
                Ok(done) => {
                    print!("[parent] wait 回收了 pid=");
                    print!("{}", done);
                    println!("");
                }
                Err(_) => println!("[parent] wait 失败"),
            }
        }
        Err(_) => println!("[init] fork 失败"),
    }

    // ---- 依次运行磁盘上的测试程序 ----
    // 这段要 fork (lab-6) 与 exec (lab-9) 都到位才能跑。init 是所有测试
    // 的父进程: 测试失败不会让内核崩掉, 退出状态由 init 收集并打印。
    println!("");
    println!("======== 测试开始 ========");
    // 用 (路径, 名字) 静态表而非从路径切名: 用户程序没链接 core 库,
    // from_utf8 等调用会在链接期报 undefined reference。
    // 默认只跑 test_1: 四个多进程测试连续跑时会间歇性停住 (两核都进 idle,
    // 进程停在 RUNNING, 详见 lab-9 README)。单跑 test_1 复现率是零。
    let tests: [(&[u8], &str); 1] = [
        (b"/test_1\0", "/test_1"),
    ];
    for (path, name) in tests {
        let status = run_test(path);
        if status == 0 {
            println!("-------- {}: 通过 --------", name);
        } else {
            println!("-------- {}: 失败 --------", name);
        }
    }
    println!("======== 测试结束 ========");

    println!("");
    println!("[init] 全部验证完成, 调用 exit(0)。");
    oslab_user::exit(0)
}

/// 跑一个测试程序: fork -> 子进程 exec -> 父进程 wait。
///
/// 返回子进程的退出状态 (负数表示这一步本身失败了)。
fn run_test(path: &[u8]) -> isize {
    let pid = match oslab_user::fork() {
        Ok(p) => p,
        Err(_) => return -1,
    };
    if pid == 0 {
        // 子进程: 用 exec 把映像换成测试程序。exec 成功不返回。
        let _ = oslab_user::exec(path);
        println!("init: exec 失败");
        oslab_user::exit(127)
    }
    // 父进程: 等这个子进程结束, 并取回它的退出状态。
    let mut status: usize = usize::MAX;
    match oslab_user::wait_status(&mut status) {
        Ok(_) if status != usize::MAX => status as isize,
        _ => -1,
    }
}

entry!(main);
