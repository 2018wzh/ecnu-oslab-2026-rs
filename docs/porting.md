# 移植指南: 如何加一块新板子

> 这份文档是一份**可执行的清单**, 不是一个概念介绍。
> 它回答一个具体问题: "我手上有一块新的 RISC-V 板子, 要让它跑起
> 这个内核, 我需要抄哪些数字、从哪里抄、抄完怎么验证?"
>
> 这是"不使用设备树"这个设计选择的**补偿措施**: 因为机器信息是
> 编译期常量, 所以必须有一份明确的人工核对流程。代价是重抄一遍
> 地址, 收益是每个常量都能被人在手册上核对, 并且很多抄错会在
> 编译期被断言拦下。

---

## 0. 总览: 需要改的文件

假设新板子叫 `mypad` (SoC 是 `XYZ123`), 你要做的是:

```text
  1.  crates/hal/Cargo.toml                加一个 feature: platform-mypad
  2.  crates/hal/src/platform/mod.rs       加两行 (mod + pub use + PLATFORM 别名)
  3.  crates/hal/src/platform/mypad.rs     新文件: 一份 Platform 的 const 实例
  4.  crates/drivers/Cargo.toml            把 feature 转发给 hal
  5.  crates/kernel/Cargo.toml             把 feature 转发给 hal 与 drivers
  6.  configs/riscv64-mypad.toml           新文件: 链接地址与交付方式
  7.  crates/kernel/build.rs               stack_slots_for() 里加一行 ncpu
  8.  docs/ports/mypad.md                  地址清单 (供核对)
  9.  (如果需要新的设备型号) crates/drivers/src/...   新的驱动
```

**注意这个清单里没有 `crates/kernel/src/`。** 如果移植过程中你发现
必须去改 `kernel/` 里的任何一行, 说明这一层抽象漏了 —— 那正是
本仓库要避免的情况。请把它当成一个**信号**而不是障碍:
它意味着某个平台事实被写进了 OS 语义。

---

## 1. 第一步: 收集机器的信息

### 1.1 需要哪些数字

| 项目 | 说明 | 从哪里找 |
|---|---|---|
| DRAM base / size | 物理内存的起点与容量 | SoC 数据手册的 "Memory Map" 章节 |
| kernel load addr | 固件跑完之后跳转到的地址 | 固件文档 (OpenSBI/U-Boot) 的启动约定 |
| firmware base | 固件自己占用的起点 | 同上 |
| ncpu | 参与运行内核的 hart 数量 | SoC 手册的 "CPU" 章节 |
| hart range (min/max) | 内核可用的 hart id **闭区间** | 同上, 注意监控核 |
| boot hart | 固件把控制权交给谁 | 固件文档 |
| PLIC base / size | 中断控制器 | SoC 手册 |
| CLINT base | 定时器 (如果可访问) | SoC 手册 |
| timer interval | 时钟中断间隔 (tick) | 由 timebase 频率算出 |
| timer kind | 时间源走 CLINT 还是纯 SBI | 由硬件能力决定 |
| UART base / irq / clock | 串口 | SoC 手册 |
| block device | 块设备类型与地址 | SoC 手册 |

### 1.2 从哪里找: 三条途径

**途径 A: 手上那台 QEMU 机器 (最快的验证方式)**

如果新板子在 QEMU 里有模型, 可以直接让 QEMU 把它的设备树 dump
出来核对:

```bash
qemu-system-riscv64 -machine <model>,dumpdtb=/tmp/my.dtb -m 128M -nographic
dtc -I dtb -O dts /tmp/my.dtb | less
```

> **注意**: 我们用这个 DTB **只是为了核对常量**, 运行期内核并不解析
> 它。这是一个"用 DTB 当手册"的用法, 与"用 DTB 当运行时输入"是
> 两件完全不同的事。

**途径 B: SoC 数据手册**

搜 "Memory Map" 与 "UART"/"PLIC"/"CLINT" 章节。对 JH7110 来说,
VisionFive2 的 `platform/visionfive2.rs` 里的注释已经记录了出处。

**途径 C: 已有的工作内核**

Linux 的设备树 (`arch/riscv/boot/dts/<vendor>/`) 是最可靠的一手
来源之一 —— 它被真实的硬件验证过。同样地: 用它当**手册**。

### 1.3 抄完必须核对的三件事

1. **hart 区间的起点是不是 0。** 很多 SoC 把 hart 0 留给监控核
   (JH7110 就是这样)。抄错的表现是"N 核系统只起来 N-1 个核",
   **不报任何错**。
2. **UART 的输入时钟。** 抄错的表现是**串口输出乱码**, 而因为
   内核确实在运行, 极容易误判成代码逻辑问题。
3. **timebase 频率。** 抄错的表现是"进程切换明显变慢"或"变快",
   同样不报错。

这三个是移植时最常见的三类 bug, 而它们的共同点是**不崩溃**。

---

## 2. 第二步: 写 platform 文件

复制 `crates/hal/src/platform/qemu_virt.rs` 作为起点, 然后:

### 2.1 必须提供全部字段

`Platform` 是一个 struct, 所以:

* 少一个字段 -> 编译错误, 错误指向你的文件;
* 多一个字段 -> 编译错误 (struct 字面量不允许多余字段);
* 字段类型不对 -> 编译错误。

也就是说, **"两个平台提供完全相同的描述"是类型系统强制的**,
而不是一句口头约定。这是本仓库相对 C 版本的主要改进之一:
C 版本里"某个平台忘了定义 `SDIO_BASE`"这件事, 除非恰好有代码
用到它, 否则无法被发现。

### 2.2 加上属于你这块板子的编译期断言

每个 platform 文件末尾的 `const _: () = { ... }` 块不是装饰, 而是
把手册上的规则变成编译器能检查的东西。至少要有:

```rust
const _: () = {
    // 1. hart 区间与 ncpu 必须自洽。
    //    这一条如果被删掉, "只起来 N-1 个核"的 bug 就会复现。
    assert!(MYPAD.harts.count() == MYPAD.ncpu);

    // 2. 启动 hart 必须落在区间内。
    assert!(MYPAD.harts.contains(MYPAD.boot_hart));

    // 3. 内存布局自洽: 内核在 DRAM 里, 且不与固件重叠。
    assert!(MYPAD.kernel_base > MYPAD.firmware_base);
    assert!(MYPAD.dram().contains(MYPAD.kernel_base));

    // 4. 页表映射要求 4 KiB 对齐。
    assert!(MYPAD.dram_base % 4096 == 0);
    assert!(MYPAD.kernel_base % 4096 == 0);
    assert!(MYPAD.uart0_base % 4096 == 0);
    assert!(MYPAD.plic_base % 4096 == 0);

    // 5. 块设备类型与填写的地址必须一致。
    //    这条防的是"复制粘贴另一个平台的文件, 忘了改 block 字段"。
    assert!(matches!(MYPAD.block, BlockKind::VirtioMmio));
    assert!(MYPAD.virtio0_base != 0);
};
```

**每一行断言都对应一类真实发生过的 bug。** 如果你的板子有额外的
约束 (例如"某个外设只能在某个窗口内"), 也写成断言。

### 2.3 例子: 从 QEMU 到 VF2 的五处差异

并排读 `qemu_virt.rs` 与 `visionfive2.rs`, 差异只有五处:

| 项目 | QEMU virt | VisionFive2 | 影响 |
|---|---|---|---|
| DRAM 基址 | `0x80000000` | `0x40000000` | 页表映射、分配器范围 |
| hart 区间 | `0..1` (含 0) | `1..4` (**不含 0**) | 启动校验、栈的索引 |
| PLIC 基址 | `0x0c000000` | `0xc000000` | 驱动不变, 只换参数 |
| 定时器 | CLINT 可读 | 全走 SBI | 驱动**选型**不同 |
| 块设备 | VirtIO-MMIO | DW MSHC (SD) | 驱动**实现**不同 |

前四项是**参数**差异, 只有最后两项是**实现选型**差异。而
`kernel/` 需要知道的只有"有几种块设备"这一件事 —— 而且是通过
trait 拿到的, 不是通过 `#[cfg]`。

---

## 3. 第三步: 接上 feature 与配置

### 3.1 feature 链

feature 必须从 `kernel` 一路转发到 `hal`:

```toml
# crates/hal/Cargo.toml
[features]
platform-mypad = []

# crates/drivers/Cargo.toml
[features]
platform-mypad = ["oslab-hal/platform-mypad"]

# crates/kernel/Cargo.toml
[features]
platform-mypad = ["oslab-hal/platform-mypad", "oslab-drivers/platform-mypad"]
```

`hal/build.rs` 会强制"恰好选中一个平台": 一个都不选或选了两个
都会在**编译前**报错, 而且错误信息是"请用 cargo xtask build
--config <name>"这样一句人能看懂的话, 而不是
"cannot find value UART0_BASE"。

### 3.2 platform 模块注册

```rust
// crates/hal/src/platform/mod.rs
#[cfg(feature = "platform-mypad")]
mod mypad;
#[cfg(feature = "platform-mypad")]
pub use mypad::*;

// 以及 PLATFORM 别名 (两个 cfg 分支里恰好有一个生效):
#[cfg(feature = "platform-mypad")]
pub const PLATFORM: Platform = MYPAD;
```

`PLATFORM` 这个别名是让上层写 `platform::PLATFORM` 而**不需要任何
`#[cfg]`** 的关键。

### 3.3 configs/*.toml

```toml
name = "riscv64-mypad"
description = "MyPad / XYZ123, U-Boot FIT image"

arch = "riscv64"
platform = "mypad"
boot = "uboot"

target = "riscv64gc-unknown-none-elf"
kernel_load_addr = 0x40200000   # 必须与 platform/mypad.rs 的 kernel_base 相等

[uboot]
load_addr = 0x40200000
entry_addr = 0x40200000
boot_command = "bootm ${kernel_addr_r}"
deploy_hint = "见 docs/board-deploy.md"
```

**`kernel_load_addr` 必须与 `platform/mypad.rs` 的 `kernel_base`
完全相等。** 这两处重复是刻意的 (理由见 `docs/architecture.md` 的
4.7 节), 而它们的一致性会在三个地方被检查: 链接脚本的 `ASSERT`,
`xtask` 构建时的 `nm` 检查, 以及内核启动时的 `config_selfcheck`。

### 3.4 栈槽位数

`crates/kernel/build.rs` 里的 `stack_slots_for()` 需要加一行:

```rust
let ncpu = match platform {
    "qemu-virt" => 2,
    "visionfive2" => 4,
    "mypad" => 4,          // <-- 加这里
    other => { ... }
};
```

这个重复也是刻意的, 而且它同样被守着: `build.rs` 生成的
`CONFIG_STACK_SLOTS` 会被内核启动时的自检与
`oslab_hal::platform::PLATFORM.ncpu` 对比。不一致就明确报错。

---

## 4. 第四步: 验证 (按顺序做, 不要跳)

### 4.1 编译

```bash
cargo xtask build --config riscv64-mypad
```

**期望**: 编译通过, 并且最后打印

```text
[ok] _entry = 0x40200000 (与配置一致)
```

如果这一步报 `_entry` 的地址不对, 说明链接脚本没被用上, 或者
`kernel_load_addr` 与 `kernel_base` 不一致 —— 错误信息里会直接
指出该检查哪几个文件。

### 4.2 检查地址清单

```bash
cargo xtask info --config riscv64-mypad
```

把输出与 `docs/ports/mypad.md` 里的表格, 以及你从手册抄下来的
原始笔记**逐项对照**。这是"不使用设备树"这个取舍要求的人工核对
步骤, 不能省。

### 4.3 在 QEMU 里跑 (如果有模型)

```bash
cargo xtask run --config riscv64-mypad --timeout 5
```

**期望看到**:

```text
[oslab-rs] kernel entry reached (SBI console)     <- 证明 CPU 活着
...
[oslab-rs] uart16550 ready @ 0x... divisor=...    <- 证明地址与时钟对
[oslab-rs] hart N (cpu M) online, sp=0x...        <- 证明多核启动对
[oslab-rs] smp: requested ... 2 now online
[oslab-rs] boot complete. entering idle loop.
```

**逐行定位问题**:

| 现象 | 原因 |
|---|---|
| 一行都没有 | 加载地址不对, 或者 `satp` 相关的问题 |
| 只有 "kernel entry reached" | platform 的 UART 地址或时钟抄错了; 或者 UART 驱动自检失败 |
| UART 那行是乱码 | `uart0_clock` 抄错了 (最常见) |
| "hart N online" 少了几行 | hart 区间抄错 (检查 `min`/`max`) |
| 卡在自检的 FATAL 信息上 | 按它打印的提示去改 (它会指出具体文件) |

### 4.4 真机

1. ```bash
   cargo xtask image --config riscv64-mypad
   ```
   这会生成 `target/riscv64-mypad/kernel.itb`, 并且:
   * 自校验 FDT 结构;
   * 用 `fdtget` (libfdt) 交叉验证 8 个关键属性;
   * 检查 `type=kernel`, `arch=riscv`, `os=linux`, `compression=none`。

2. 按 `docs/board-deploy.md` 的步骤把 `.itb` 拷到 SD 卡并在
   U-Boot 里 `bootm`。

3. **第一次启动一定要接串口看输出。** 真机上没有输出时,
   你无法区分"内核没跑"和"内核跑了但串口不对"。

---

## 5. 常见陷阱速查表

| 陷阱 | 症状 | 检查点 |
|---|---|---|
| hart 区间起点当成 0 | 少起来一个核, 无报错 | `harts.min` 是不是 1 |
| UART 时钟抄错 | 串口乱码 | `uart0_clock` 与手册 |
| 加载地址与链接地址不一致 | 静默跑飞 / 半句话后卡死 | `kernel_load_addr` == `kernel_base` |
| 内核与固件重叠 | 启动后立刻异常 | `kernel_base > firmware_base` |
| 时间源用了不可访问的 CLINT | 时钟中断后卡死 | `timer_kind` 改成 `Sbi` |
| 定时器间隔用了另一块板子的值 | 调度明显变慢/变快 | `timer_interval` 与 timebase |
| 块设备类型与地址不匹配 | 读到垃圾 | `block` 与 `virtio0_base`/`sdhci_base` |
| SD 初始化时钟不是 400 kHz | 卡完全不应答 | `sdhci.rs` 的 `SD_INIT_CLOCK_HZ` |
| 在新平台上直接抄 `ncpu` 到 build.rs 但忘了 hal | 栈槽位数不对 | 启动自检会报 |

---

## 6. 什么时候需要写一个新驱动

`platform/` 只描述"设备在哪"和"它是什么型号"。**设备型号**用一个
枚举表达 (`BlockKind`、`TimerKind`)。如果你需要一个新的枚举变体,
那说明这是一类新设备, 需要在 `drivers/` 里写实现:

1. 在 `drivers/src/<category>/` 下新建模块;
2. 如果它属于已有类别 (例如又一个块设备), 实现那个类别的 trait
   (`BlockDevice`) —— 于是文件系统不需要任何改动;
3. 在 `platform::<Kind>` 枚举里加变体, 并在平台文件里填上对应的
   地址字段;
4. 在 `drivers` 的初始化路径里根据 `plat.block` (或类似字段)
   分发 —— 用 `match`, 不要用 `#[cfg]`。

**加变体而不是加 `#[cfg]` 的理由**: `match` 的穷尽性会让编译器
指出所有需要更新的地方; `#[cfg]` 不会。

---

## 7. 这份清单为什么存在 (再强调一次)

如果我们用了设备树, 加一块板子就只是"把 DTB 编进 U-Boot"的事,
没有这份文档。那份便利是用下面的代价换来的:

* 运行期需要一个几百行的 fdt 解析器;
* 地址错误的表现从"编译期断言失败"变成"运行期静默无输出";
* 学生调试的第一个问题从"我的常量抄对了吗"变成"我的解析器
  写对了吗"。

对本课程而言, 这些代价不划算 —— 因为我们只需要支持**少数几台
已知的机器**, 而且学生的时间应该花在 OS 语义上, 不是 FDT 上。

这份清单就是那个取舍的**另一半**: 手工的代价被明确写下来、
可执行、并且尽可能被编译期断言自动化。
