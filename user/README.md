# `user/` — 用户程序 (lab-4 之后使用)

这里放**用户程序**与它们的运行时 (`src/runtime.rs`)。每个 `src/bin/*.rs`
是一个独立 ELF, 由 `cargo xtask build` 用 cargo + `rust-lld` 直接链接
(见 `xtask/src/user.rs` 与 `user/build.rs`), 最后进磁盘镜像, 由内核在
`exec` 时装入。

## 为什么用户程序是一个独立的 crate

用户程序与内核跑在不同特权级、用不同链接地址、由不同入口进入:

* **用户程序** (`U-mode`): 链接基址 `0x1000`, 入口 `_user_start`
  (见 `oslab_uapi::USER_ENTRY_SYMBOL`), 除了 `ecall` 不能执行特权指令;
* **内核** (`S-mode`): 加载地址 `0x80200000` (QEMU) 或 `0x40200000`
  (VisionFive2), 入口 `_entry`, 可执行全部 CSR / 特权指令。

分成两个 crate 之后, 依赖方向由 Cargo 强制: `内核 -> uapi <- 用户程序`,
双方共享的只有 ABI 定义 (`crates/uapi/`), 没有任何实现代码。

## ABI 契约

用户程序与内核之间的**唯一**契约是 `crates/uapi`:

```rust
// 用户程序侧
use oslab_uapi::{Syscall, encode_ret, decode_ret};

fn write(fd: usize, buf: &[u8]) -> Result<usize, SysError> {
    let raw = unsafe { syscall3(Syscall::Write as usize, fd, buf.as_ptr() as usize, buf.len()) };
    decode_ret(raw)
}
```

运行时 (`src/runtime.rs`) 里保留一份 ABI 数值副本, 并配了编译期断言,
防止与 `crates/uapi` 漂移。