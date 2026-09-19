//! `arch::riscv64` — RISC-V 的用户态系统调用机制。
//!
//! 这里是本运行时里**唯一**与 CPU 架构相关的一段: `syscall_raw`。
//! 它把"参数放进 a0-a2、调用号放进 a7、执行 `ecall` 陷入 S-mode"这条
//! RISC-V 约定封装成一个可被上层调用的函数。换架构时只需提供一个
//! 相同签名的实现 (例如 aarch64 用 `svc #0`), 上层封装全部复用。

/// 发起一次 RISC-V 系统调用 (原始形式, 返回内核给的原始值, 负数 = 错误)。
///
/// # 寄存器约定 (来自 `docs/abi-spec.md` 第 1 节)
///
/// ```text
///   a7      = 系统调用号
///   a0..a2  = 参数 (本课程最多 3 个)
///   ecall   -> 陷入 S-mode
///   a0      = 返回值 (负数表示错误)
/// ```
///
/// 公开是为了调试 ABI: 上层把返回值解码成 `Result`, 而这里能看到原始值,
/// 以区分"内核返回了错误码"与"我们解码错了"。
pub fn syscall_raw(num: usize, a0: usize, a1: usize, a2: usize) -> isize {
    let ret: isize;
    // SAFETY: `ecall` 是用户态唯一能陷入内核的指令。副作用是内核可能
    // 读写用户缓冲区, 故声明内存 clobber。
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") a0 => ret,
            in("a1") a1,
            in("a2") a2,
            in("a7") num,
            // 不写 `nomem`: Rust 默认认为汇编可读写任意内存, 正是我们要的
            // (内核会读写用户缓冲区)。写 `nomem` 等于保证"不碰内存",
            // 编译器会允许把缓冲区写入推迟到 ecall 之后, 内核就读到旧数据。
            //
            // `nostack`: 本指令不压栈, 内核用自己的栈, 返回时用户 sp 不变。
            //
            // `clobber_abi("C")` 不能省: 系统调用是跨特权级调用, 内核返回时
            // 只保证 callee-saved 寄存器不变, caller-saved (t0-t6/a1-a7) 都可能被改。
            // 不声明, 编译器会假设 ecall 只动 a0, 于是把值放 t 寄存器跨过 ecall,
            // 那些值会被 trap 处理覆盖 —— 曾导致 fd 变 2、指针被算成 0 等迷惑症状。
            options(nostack),
            clobber_abi("C"),
        );
    }
    ret
}