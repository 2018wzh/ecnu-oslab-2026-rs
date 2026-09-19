# LAB-1: 机器启动

## 1. 代码组织结构
```
ecnu-oslab-2026-rs
├── Cargo.toml / rust-toolchain.toml   Rust 2024 edition
├── configs/                           配置档案: arch + platform 两维
│   ├── riscv64-qemu-virt.toml
│   └── riscv64-visionfive2.toml
├── xtask/                             构建系统 (cargo xtask build|run|disk ...)
├── crates/
│   ├── hal/                           硬件抽象层 (arch + platform)
│   │   ├── build.rs                   编译期校验: 恰好一个 arch + 恰好一个 platform
│   │   └── src/
│   │       ├── arch/riscv64/          CPU 语义: CSR / SBI / trap / 启动
│   │       │   ├── boot.rs            内核第一条指令 (global_asm)
│   │       │   ├── cpu.rs             hartid / cpuid 换算
│   │       │   ├── csr.rs             CSR 读写
│   │       │   ├── sbi.rs             SBI 调用 (固件控制台等)
│   │       │   ├── time.rs            rdtime
│   │       │   └── trap.rs            最小 trap 兜底点
│   │       ├── platform/              机器语义: 地址、中断号、CPU 拓扑
│   │       │   ├── qemu_virt.rs
│   │       │   └── visionfive2.rs
│   │       └── putchar.rs             极简输出 (panic 路径也要能用)
│   ├── drivers/                       设备协议
│   │   ├── serial/uart16550.rs        两个平台复用的 16550 驱动
│   │   └── mmio.rs                    MMIO 读写封装
│   └── kernel/                        OS 语义 (不知道自己在哪台机器上)
│       └── src/
│           ├── main.rs                启动流程 + 模块声明
│           ├── console.rs (TODO)      格式化输出
│           └── panic.rs               panic 处理器
└── user/                              用户态程序 (本阶段还没有)
```

## 2. 实验核心目标

让内核在 QEMU(OpenSBI) 与 VisionFive2(U-Boot) 两块平台上启动，并通过
真实的 16550 串口打印出板级参数。

## 3. 机器是怎么启动的

### 3.1 从固件到内核

```
   上电
    │
    ├─ M-mode 固件 (QEMU: OpenSBI / VF2: U-Boot 内部的 OpenSBI)
    │     初始化 DRAM、时钟、串口……
    │
    ├─ 跳转到 S-mode 内核入口 (hal/arch/riscv64/boot.rs)
    │     ★ 从这里开始是本课程要写的代码
    │
    └─ kernel::main()
```

固件已经做完了 M-mode 的脏活，所以内核一上来就在 S-mode。代价是 S-mode 不能
直接写 `mtimecmp`，设置时钟中断必须通过 SBI 请固件代劳（lab-3 会看到）。换来
的是同一份代码能在所有 RISC-V 平台上跑。

### 3.2 固件交给我们的寄存器约定

| 寄存器 | 含义 |
|---|---|
| `a0` | 启动核的 hartid |
| `a1` | 设备树 (DTB) 的物理地址 —— 本课程不使用 DTB，约定保持一致 |
| `satp` | 0（分页未开启） |
| `sp` | **未定义**！内核必须自己建立栈 |

最后一条最容易出事：内核的第一条指令执行时没有可用的栈。所以 `boot.rs` 的
第一件事不是 `call`，而是先算出一个栈地址写进 `sp`。

### 3.3 两台机器传给我们的 hartid 不一样

| | QEMU virt | VisionFive2 |
|---|---|---|
| DRAM 起点 | `0x80000000` | `0x40000000` |
| 内核加载地址 | `0x80200000` | `0x40200000` |
| 启动核 hartid | **0** | **1** |
| 可用 hart 区间 | `[0, 1]` | `[1, 3]` |
| UART0 地址 | `0x10000000` | `0x10000000` |
| UART0 中断号 | 10 | **32** |
| 输入时钟 | 3 686 400 Hz | 24 000 000 Hz |

VisionFive2 的 hart 0 被监控核占用，所以它的启动核是 hartid **1**。

内核里到处需要的是"第几个核"（`cpuid`，从 0 开始），固件给的是 hartid。
换算必须由平台层提供（`boot_hart`），因为"哪些 hart 归内核用"是这台机器的
事实。

写死 `if hartid == 0` 的后果：在 VisionFive2 上没有任何核会执行启动核的
初始化代码 —— 现象是内核一句话都不打印。

## 4. 串口：本实验唯一的设备

### 4.1 为什么是 16550

16550 是 1987 年的 PC 串口芯片，但几乎所有 RISC-V 开发板都集成了兼容核。
差异全部来自平台常量：

```rust
let clock = PLATFORM.uart0_clock;   /* 分频用 */
let irq   = PLATFORM.uart0_irq;     /* 注册中断用 */
let base  = PLATFORM.uart0_base;    /* 寄存器窗口 */
```

驱动源码一行都不用改。

### 4.2 波特率分频

```
   除数 = 时钟频率 / (16 * 波特率)

   115200 波特率下:  QEMU 3686400/(16*115200) = 2
                     VF2  24000000/(16*115200) = 13
```

分频算错的症状是输出全是乱码 —— 而内核"确实在运行"，很容易误判成逻辑问题。

## 5. 具体任务

本分支里下面这些函数体是空的（`{ }`）：

| 文件 | 函数 |
|---|---|
| `crates/kernel/src/console.rs` | `print_hex_bare`：不带 `0x` 前缀的十六进制 |
| `crates/kernel/src/console.rs` | `print_dec`：十进制 |

这是 printf 的地基：后面每个实验里你都会大量用它打印地址、进程号、inode 号。
一个会输出错数字的格式化函数会让所有调试输出都不可信。

### 5.1 `print_hex_bare` 的要点

- 64 位最多 16 个十六进制数字，用一个栈上的 `[u8; 16]` 缓冲区
- 取模天然是从低位开始的，而输出要从高位到低位 —— 所以先存后倒序输出
- **`0` 要单独处理**：不处理的话循环一次都不执行，输出空字符串
- 用 `putchar::putc` 输出，不要用 `core::fmt`（体积大得多）

### 5.2 `print_dec` 的要点

和 `print_hex_bare` 结构完全一样，只是进制换成 10。同样要注意 `0` 与
`usize::MAX` 两个边界。

### 5.3 现在你应该看到什么

实现之前，启动横幅里的数字位置是空白：

```
[oslab-rs] uart16550 ready @ 0x divisor= irq=
```

实现之后：

```
[oslab-rs] uart16550 ready @ 0x10000000 divisor=2 irq=10
```

这就是本阶段的验收标准：数字出现了，而且与平台常量一致。

## 6. 测试

### 6.1 QEMU

```bash
cargo xtask run --config riscv64-qemu-virt
```

### 6.2 VisionFive2

```bash
cargo xtask build --config riscv64-visionfive2
cargo xtask image --config riscv64-visionfive2     # 生成 kernel.itb
```

输出应与 QEMU 结构完全相同，只有数值不同：`DRAM 0x40000000`、
`kernel 0x40200000`、`boot hart = 1`、`irq = 32`。

### 6.3 自检清单

- [ ] 内核的第一条指令执行时，`sp` 里是什么？我们怎么解决？
- [ ] VisionFive2 的启动核为什么是 hartid 1 而不是 0？
- [ ] `cpuid` 与 `hartid` 的区别是什么？为什么需要换算？
- [ ] 串口分频在 QEMU 上是 2、VF2 上是 13。为什么驱动源码不用改？
- [ ] 为什么 `print_dec(0)` 需要单独处理？
- [ ] 为什么 `kernel/` 里不允许出现 `#[cfg(feature = "platform-...")]`？

## 7. 关于代码仓库的维护

1. 每次实验需要在上次实验的基础上继续往下做，假设你已经完成 lab-0(master)

    那么你此时应该在 lab-0(master) 分支下使用 `git checkout -b lab-1` 命令
    创建并切换到新的分支 lab-1

    此时新建的 lab-1 会继承 lab-0(master) 的内容，但你对 lab-1 的修改不会
    影响到 lab-0

    以此类推，当你从 lab-1 开始走到 lab-9 时，你会获得越来越完整和强大的内核

2. 你的代码仓库应该由 **代码 + Markdown文档** 两部分构成

    文档内容不做明确要求，你有很高的自由度决定写什么和写多少

    提供一些建议：

    - 本次实验新增了哪些功能，实现了什么效果

    - 对本次实验中某个过程的理解和思考

    - 本次实验和之前的实验构成什么样的逻辑联系

    - 本次实验花费的时间, 你和队友的贡献分别是什么

    - 可以使用 markdown 的分层分点来增加条理性，便于别人阅读和抓住重点

    **总之，这是你的代码仓库，请对你自己的代码和文档负责**

    **注意，代码是继承和连续发展的, 但文档不是，每次的文档都是全新一页**

3. 提醒: 之所以要求大家维护代码仓库，是为了查看大家的提交记录

    所以请及时同步当天写的代码到线上仓库，不要攒到最后一口气提交，否则可能
    被误判为不当行为

---

## 6. 进阶目标（可选）

下面三条**不属于基本验收**：默认流程与本分支 README 的期望输出都不依赖它们。
它们的作用是把这一阶段的内核"做完整一点"——每条都只用到**本章已经给出的东西**，
不碰后面阶段的文件。每条写了"为什么值得做""思路（只说做法，不写代码）""怎么算做到"。

> 动手前先建自己的分支（例如 `git checkout -b my-advanced`）。
> **别破坏默认输出**：进阶改动如果改变了默认运行结果，后面几章的对照实验就失效了；
> 需要改默认行为时，用一个新的 `configs/` 配置或一个运行期开关把它隔开。

---

### 6.1 bootinfo：把"这次启动是怎么发生的"收进一个结构

**为什么值得做**：现在"固件怎么交接"这件事散在三处——`crates/hal/src/arch/riscv64/boot.rs` 的 `global_asm!` 里 a0/a1 的约定、
`crates/hal/src/arch/riscv64/cpu.rs` 里从 `tp` 与全局符号反推"谁启动了内核"、
以及 `configs/riscv64-qemu-virt.toml` / `configs/riscv64-visionfive2.toml` 里的 `boot` 字段。
更麻烦的是第三处：`boot = "sbi"` 与 `boot = "uboot"` 目前**只影响构建系统**（链接脚本与产物格式），
内核代码侧根本没有对应的开关，加第二个启动协议时这些假设会被各写一遍。

**思路**：
- 定义一个只描述**本次启动事实**的结构（例如 `BootInfo`）：启动 hart、DTB 物理地址、早期控制台来源、
  协议名、固件是否提供"启动其他 hart"的能力……**不要**放平台事实，那些仍然归 `crates/hal/src/platform/qemu_virt.rs` 与 `crates/hal/src/platform/visionfive2.rs`。
- 让 boot 这一维真正进入代码：给 `crates/hal/Cargo.toml` 加上 `boot-sbi` / `boot-uboot` 两个 feature，
  像 `crates/hal/build.rs` 现在校验 arch/platform 那样校验"恰好选中一个"，再由启动入口各自填这个结构。
- 汇编现在只把 hartid 搬进 `tp`：a1（DTB 地址）是 caller-saved，必须在进 Rust 之前就存进一个固定的 `.data` 槽位，
  否则第一次函数调用就没了——这一条与启动汇编里"抽签哨兵必须放 `.data`"是同一个理由。
- `crates/kernel/src/main.rs` 与其它代码不再直接读启动全局符号或 `tp`，只读这个结构；
  `crates/hal/src/arch/riscv64/cpu.rs` 的 `hartid()` / `cold_boot_hart()` 也改成从它取。
- 两个现有协议都要填出完整内容，并打印一行便于对照。

**怎么算做到**：两个现有配置下结构内容都正确；`crates/kernel/src/` 里不再出现 a0/a1 的原始语义与启动全局符号；
再加一个 boot 维度时 `crates/kernel/` 一行都不用改。

**涉及**：`crates/hal/src/arch/riscv64/boot.rs`、`crates/hal/src/arch/riscv64/cpu.rs`、`crates/hal/Cargo.toml`、`crates/hal/build.rs`、`configs/riscv64-qemu-virt.toml`、`configs/riscv64-visionfive2.toml`、`crates/kernel/src/main.rs`　**难度**：★★☆

### 6.2 dtb 动态发现：平台事实的第二个来源

**为什么值得做**：`crates/hal/src/platform/qemu_virt.rs` 里的 DRAM 基址、UART 地址、hart 区间现在都是**编译期常量**——
这正是本仓库"不用设备树"的刻意取舍（理由写在 `crates/hal/src/platform/mod.rs` 顶部的长注释里）。
而 a1 里一直拿着 DTB 的物理地址，启动汇编却没有保存它，等于把这个事实白白丢掉了。
做完这条，你会同时理解"为什么需要 platform 层"和"平台层的事实还能从哪来"，也才有资格判断那个取舍在什么条件下不再成立。

**思路**：
- 新增的 `crates/hal/src/arch/riscv64/fdt.rs`：一个最小的 FDT 遍历器——校验 header 魔数，
  再按结构块走（开始节点 / 属性 / 结束节点），取出三件事：`/memory` 的 reg（DRAM 基址与大小）、
  `/cpus` 的 timebase-frequency 与 hart 列表、串口节点的 reg（UART 基址）。
- 启动汇编把 a1 保存下来（见 6.1），Rust 入口把它作为物理地址传给 FDT 遍历器；恒等映射下这地址可以直接读。
- 与 `crates/hal/src/platform/mod.rs` 的编译期值**逐个对账**：一致就打印"一致"，不一致就以 DTB 为准并打印警告；
  对账逻辑写成一个小函数，两个平台共用。
- 注意 FDT 是**大端**，而且字段偏移都来自外来数据：每一处长度与偏移都要做边界检查，节点深度要设上限（防止成环）；
  Rust 里读大端整数用 `u32::from_be_bytes` 即可，比手写移位更不容易错。
- 可以先把 DTB 导出到文件离线观察：`qemu-system-riscv64 -machine virt,dumpdtb=/tmp/virt.dtb`。

**怎么算做到**：两个平台都能打印"从 DTB 读到的 DRAM/CPU/UART"三行，并给出与编译期值的一致/不一致结论；
DTB 被截断或魔数不对时能报错而不是崩。

**涉及**：`crates/hal/src/arch/riscv64/boot.rs`、`crates/hal/src/platform/mod.rs`、`crates/hal/src/platform/qemu_virt.rs`、`crates/hal/src/platform/visionfive2.rs`　**难度**：★★★

### 6.3 UEFI：第三个启动协议（可以先只做骨架）

**为什么值得做**：这条检验的是本项目的核心设计目标——"启动维度可替换"是否真的成立。
UEFI 与现有两种协议差别极大：产物是 PE/COFF、靠 Boot Services 拿内存与输出、
退出服务后没有任何固件回调可用，而且它连链接脚本与入口约定都要另换一份。

**思路**（分两级，建议先做 L1）：
- **L1 只做骨架**：照 `configs/riscv64-qemu-virt.toml` 的写法新增一份 `boot = "uefi"` 的配置
  （指向新的链接脚本与产物格式），给 `crates/hal` 加一个 `boot-uefi` feature 与一份入口实现，
  把 6.1 的 bootinfo 填好，再用 `xtask` 把 ELF 转成 PE/COFF。
  目标是在 QEMU 的 OVMF 固件下被加载并打印出第一行。全程**不改 `crates/kernel/`**，这正是要证明的东西。
- **L2 才做真正的引导**：用 Boot Services 拿内存图、配置输出，`ExitBootServices` 之后再跳内核。
- 关键差异提前想清楚：UEFI 下没有 SBI 控制台，早期输出从哪来？
  （这正是 bootinfo 里"早期控制台来源"那一项存在的理由。）
- 构建侧的落点：`xtask/src/build.rs` 现在按配置里的 `boot` 字段决定产物格式，UEFI 需要在那里加一条分支与一个新的镜像生成步骤；
  `xtask/src/fit.rs` 是同类东西（U-Boot FIT）的现成范例，可以照着写。

**怎么算做到**：L1 在 OVMF 下能看到内核第一行输出，且 `git diff --stat crates/kernel/` 为空；
L2 能把内存图交给物理页分配器，并说明它与 `crates/hal/src/platform/*.rs` 里那套常量的关系。

**涉及**：`configs/riscv64-qemu-virt.toml`、`configs/arch/riscv64.toml`、`crates/hal/Cargo.toml`、`crates/hal/src/arch/riscv64/boot.rs`、`xtask/src/build.rs`、`xtask/src/fit.rs`　**难度**：★★★（L1 ★★☆）
