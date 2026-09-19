# LAB-1: 机器启动

**前言**

本次实验从机器启动开始。你需要让多个 CPU 进入内核，完成初始化，通过串口输出信息。QEMU 使用 OpenSBI，VisionFive2 使用 U-Boot/OpenSBI；固件已经完成进入 S-mode 之前的工作。内核运行于 `no_std` 环境。

## 1. 代码组织结构

```text
Cargo.toml、rust-toolchain.toml     工作空间与工具链 (NEW)
configs/                           平台与架构配置 (NEW)
crates/hal/src/
  arch/riscv64/entry.S             入口与每核初始栈 (NEW)
  arch/riscv64/boot.rs             Rust 启动工作 (TODO)
  arch/riscv64/cpu.rs、csr.rs      CPU 身份与中断开关 (NEW)
  arch/riscv64/sbi.rs              固件调用 (NEW)
  arch/riscv64/kernel.ld.in        链接布局 (NEW)
  platform/                       硬件参数 (NEW)
crates/drivers/src/serial/         UART 驱动 (NEW)
crates/kernel/src/
  main.rs                         双核主流程 (TODO)
  console.rs、panic.rs             串口连接与紧急输出 (NEW)
  print.rs                        输出适配 (TODO)
  lock/spinlock.rs                自旋锁 (TODO)
xtask/                            构建与镜像工具 (NEW)
```

`NEW` 是教师完整提供的新代码，`TODO` 是学生任务；`CHANGE` 表示修改旧文件。HAL 描述架构与平台，drivers 实现设备协议，kernel 组织内核功能。宿主 xtask 使用 clap 解析命令行，serde/toml 读取配置；这些依赖不进入裸机内核。

## 2. 实验核心目标

完成双核启动，进入 `kernel_main()` 并输出：

```text
cpu 0 is booting!
cpu 1 is booting!
```

顺序可以不同，但每核只打印一次，字符不能交错。QEMU 默认两核；VisionFive2 使用 hart 1～4。

## 3. 具体任务

| 文件 | 本章任务 |
|---|---|
| `crates/hal/src/arch/riscv64/boot.rs` | `start` |
| `crates/kernel/src/main.rs` | `kernel_main`：初始化、启动其他核、同步 |
| `crates/kernel/src/print.rs` | `Writer::write_str`、`print` |
| `crates/kernel/src/lock/spinlock.rs` | `SpinLock::init/holding/lock`、`SpinGuard::drop` |

### 3.1 机器启动本身

要想实现上述核心目标，仔细想想只需要完成两件事：

1. 让内核在 QEMU 上跑起来：`entry.S` 到 `boot.rs` 到 `main.rs`。
2. 让内核向屏幕输出字符串，也就是实现内核自己的打印功能。

阅读汇编与链接脚本，理解每核初始栈及汇编如何调用 Rust。`a0` 是 hartid；冷启动的 `a1` 是 DTB 地址，HSM 从核的 `a1` 是启动参数。

汇编完整提供栈建立、CPU 身份保存和冷启动 BSS 清零。学生完成 `start()`：调用 `csr::early_init()`，保持分页与中断关闭，安装早期异常入口，再进入 `kernel_main()`。不需要编写 M-mode 到 S-mode 切换。

主核调用 `print::init()`，通过 `cpu::start_cpu()` 启动其他核并处理 `Result`。其他核用原子变量等待初始化结果，思考发布和获取各需要什么内存顺序。

hartid 是硬件编号，cpuid 是从 0 开始的连续编号。用 `cpu::is_boot_cpu()`，不假定 hart 0。每核输出后进入 `cpu::park()`。

### 3.2 打印面临的资源竞争问题

串口是一种设备资源，输出字符串需要连续占用它。两个 CPU 同时打印 `"hello,world!"` 和 `"hello,os!"`，可能出现：

```text
# 混乱
hellohello,,world!os!
hheelllloo,,wosrld!!
# 有序
hello,world!hello,os!
hello,os!hello,world!
```

生活中的例子：公共卫生间通过“门锁”保证资源在一段时间内只被一人独占。操作系统中最简单的资源锁就是自旋锁。

```rust
{
    let _guard = PRINT_LOCK.lock();
    console::putc(b'O');
    console::putc(b'S');
} // Drop 释放锁。
```

关闭本核中断避免中断处理重复请求同一锁；原子操作避免多个 CPU 同时获取成功。教师提供嵌套的 `push_off/pop_off`，学生完成初始化、持有判断、获取与释放。

`SpinLock::UNINIT` 只提供静态存储，主核在并发使用前调用 `init()`。`lock()` 返回守卫，释放逻辑写在 `Drop`，守卫不得跨 CPU 移动。注意 Acquire/Release 顺序与提前返回时的生命周期。

### 3.3 打印适配

先读 UART 和 `console::putc()`。`core::fmt` 已提供数字与字符串格式化，学生完成 `Writer::write_str()` 将字节输出，并在 `print()` 中持有一个守卫完成整次 `write_fmt()`。

教师提供 `print!`、`println!`。一次格式化可能多次调用 `write_str()`，锁要覆盖整次打印，不能只保护片段。使用语言自带 `assert!`，panic handler 走独立紧急路径。调用 `unsafe` 时说明前提并保留 `SAFETY` 注释。

## 4. 测试

```bash
rustup target add riscv64gc-unknown-none-elf
rustup component add llvm-tools
cargo xtask build --config riscv64-qemu-virt
cargo xtask run --config riscv64-qemu-virt
cargo xtask debug --config riscv64-qemu-virt
```

用 `gdb-multiarch` 加载命令打印的 ELF，执行 `target remote :1234`。`cargo xtask --help` 查看用法。框架可编译，但未完成函数会报告 TODO 并停止。

检查独立栈、初始化一次、等待正确、每核打印一次；测试零、负数、十六进制、字符与字符串；检查多个格式化片段不会交错、守卫退出释放、中断嵌套恢复及断言失败路径。

VisionFive2 镜像需要 `dtc`：

```bash
cargo xtask image --config riscv64-visionfive2
```

将打印路径中的 `kernel.itb` 放到 FAT 分区，U-Boot 中例如：

```text
fatload mmc 1:1 ${kernel_addr_r} kernel.itb
bootm ${kernel_addr_r}
```

设备/分区号按实际环境调整，串口 115200 8N1。固件须支持 SBI HSM，冷启动只交接一个 hart，其他核由内核启动。镜像生成不等于真机验证。

## 5. 课后实验

### 5.1 并行加法

两个 CPU 对同一 `AtomicUsize` 各加一百万次。先用分开的 `load/store` 观察丢失更新，再用锁保护整个读、加一、写回。等两核完成后结果应为 `2000000`。不要无同步访问 `static mut`。比较与 `fetch_add()` 的区别，讨论锁的粒度粗细。

### 5.2 并行输出

去掉打印锁，设计测试使输出交错，再恢复比较。一次未出现交错不能证明不存在竞争。测试代码与结果可放入文档，不要留在正常启动流程。

### 5.3 进阶目标

- UEFI：了解并尝试另一种启动路径。
- DTB 动态发现：读取内存、CPU 和串口信息。
- BootInfo：统一不同启动方式交接的信息。

## 6. 关于代码仓库的维护

教师仓库配置为 `upstream` 后：

```bash
git fetch upstream
git switch -c lab-1 upstream/lab-1
# 完成并提交，例如 lab-1: implement boot and spinlock
```

进入下一实验前提交全部工作：

```bash
git fetch upstream
git switch -c lab-2 lab-1
git rebase --onto upstream/lab-2 upstream/lab-1
```

教师只增加框架，不用答案替换原实现。遇到冲突先理解双方改动。仓库应包含代码和 Markdown 文档，记录功能、思考、实验联系、耗时与队友贡献。总之，这是你的仓库，请对自己的代码和文档负责。代码连续发展，文档记录每次新工作；请及时提交并同步。
