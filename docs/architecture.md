# ECNU OSLab 2026 (Rust) — 架构说明

> 这份文档解释**为什么**仓库被组织成现在这个样子。
> 它比 `README.md` 长, 因为"怎么做"已经写在代码里了, 而"为什么"
> 是读代码读不出来的。

## 1. 一句话概括

这是一个**纯 Rust** 的裸机 RISC-V 教学内核, 一份代码同时支持
QEMU `virt` 与 VisionFive2 (JH7110) 两块机器, 用 `cargo xtask`
构建, 四个概念被严格分开:

```text
  kernel/    OS 语义      调度、VM 策略、文件系统、系统调用
  arch/      CPU/ISA 语义  CSR、Sv39 页表、trap 入口、上下文切换
  platform/  机器语义      地址、中断号、CPU 拓扑  —— 静态常量, 不用设备树
  drivers/   设备协议      PLIC、16550、VirtIO、DW MSHC
```

## 2. 四层分离, 以及一条可检验的判据

### 2.1 每一层的职责

| 层 | 知道什么 | 不知道什么 |
|---|---|---|
| `kernel/` | 什么是进程、什么是页、什么是文件 | UART 在 0x10000000、有 2 个还是 4 个核 |
| `arch/` | `sstatus.SPP` 是第 8 位、`satp` 里存的是 PPN | UART 在哪、有几块网卡 |
| `platform/` | DRAM 从哪开始、PLIC 在哪、hart 区间是 `[1,4]` | 怎么切换页表、怎么写 UART 寄存器 |
| `drivers/` | PLIC 的 claim/complete 协议、16550 的分频算法 | 谁在调用它、系统有几台机器 |

### 2.2 判据: 换板子时哪一层要改?

这是本仓库最重要的一个问题。每当你犹豫"这段代码该放哪一层",
用下面的表格回答:

| 改动 | 应该只动 | 如果动了别的地方, 说明 |
|---|---|---|
| 换一块 RISC-V 板子 (地址、中断号、核数变了) | `platform/` + `configs/` | 边界划错了: 有平台事实泄漏进了 `kernel/` |
| 换一个同样用 RISC-V 的 SoC, 但设备型号不同 | `platform/` + 新增 `drivers/` 实现 | 驱动里写死了地址, 或者 kernel 里出现了设备判断 |
| 换一个架构 (aarch64) | `arch/` + 新增 target | `kernel/` 里出现了 CSR 名字或 RISC-V 特有的数值 |
| 改调度算法 | `kernel/` | 调度器里出现了 `#[cfg(platform)]` |

**这条判据是可执行的**, 有两个层面:

1. `cargo xtask build --config riscv64-visionfive2` 会真的用另一套
   platform 常量重新编译 `kernel/`。任何写死的 RISC-V 或 QEMU 假设
   都会立刻暴露成编译错误 (或者在启动时的自检里失败)。
2. `cargo xtask check-arch` (以及 `cargo test`) 会扫描
   `crates/kernel/src/`, 拒绝 `#[cfg(feature)]`、`#[cfg(target_arch)]`、
   CSR 名字和硬编码的设备地址 (`xtask/src/architecture_rules.rs`)。

第 2 条为什么必须存在: 一条只写在文档里的规则会**缓慢地腐烂**。
某一次"就这一次例外"之后是第二次、第三次, 半年后这条规则只剩下
一句注释。**规则要么被自动检查, 要么不存在。**

### 2.3 为什么 kernel 依赖 drivers, 而 hal 不依赖 drivers

```text
  kernel   ──→ drivers ──→ hal
     │                      ↑
     └──────────────────────┘
```

* `drivers` 依赖 `hal`: 驱动需要知道设备在哪 (platform 常量), 也
  需要与 CPU 打交道 (关中断、内存屏障)。它只能通过
  `hal::arch::irq::disable()` 这样的**语义化**接口, 不能直接碰 CSR。
* `hal` **不**依赖 `drivers`: 否则 `hal` 就必须知道 PLIC、UART 的
  存在 —— 那是层次倒置。C 版本里 `drivers/irqchip/plic.c` 直接
  `#include <asm/csr.h>` 就是这类越界。
* `kernel` 依赖 `drivers` 只是为了拿到 **trait 定义**
  (`BlockDevice`) —— 依赖方向仍然是单向的, `drivers` 完全不知道
  `kernel` 存在。

**UART 的输出怎么从 drivers 走到 hal**: 用**依赖倒置**。
`hal::putchar` 定义一个函数指针, `drivers::serial` 在初始化时把
自己的实现注册进去 (`register_uart`)。于是:

```text
  drivers ──知道──→ hal        (drivers 调用 hal 的注册接口)
  hal     ──不知道→ drivers    (hal 只有函数指针)
```

## 3. 三个关键的设计决定 (以及它们各自的代价)

### 3.1 arch 用 facade, 不用 trait

**决定**: `hal::arch` 是一层 `#[cfg(target_arch)]` 的模块分发,
对外暴露一组**同名同签名的自由函数**。没有 `trait Arch`。

**为什么**:

1. 系统里永远只有**一个**架构。用 `dyn Arch` 表达它, 等于声称
   "运行时会变", 于是每次 `arch::irq::disable()` 都变成一次间接
   跳转 —— 而这些调用出现在 trap 入口和调度器的关键路径上。
2. 架构特有的类型 (`Sstatus`、`Pte`、上下文结构体) 无法在 trait 里
   表达。硬要表达就必须全部降级成 `u64`, 抽象的收益当场归零。

**代价**: 加入新架构时, 编译器会要求你提供所有同名函数, 但
**不会**告诉你"你漏了某个函数" —— 直到有代码调用它。这与 trait
的 `impl ... for` 强制完整性略有不同。可接受: 因为 `kernel/`
在两个架构上的调用点是同一份代码, 编译一次就能发现所有缺口。

### 3.2 platform 用静态常量, 不用设备树

**决定**: `platform/<board>.rs` 是一份 `Platform` struct 的 `const`
实例。运行期不解析 DTB。

**为什么** (三条, 第三条是决定性的):

1. **教学成本**: 解析 DTB 需要几百行 fdt 遍历器加边界检查, 而
   学生这个阶段要学的是调度、页表、文件系统。
2. **错误的可见性**: DTB 方案下"UART 地址读错"的表现是**静默无
   输出** —— 因为你想用来打印错误的那条路径本身就坏了。静态常量
   可以在编译期断言和启动横幅里被核对。
3. **DTB 解决的是"运行期多样性", 而我们面对的是"编译期唯一性"**。
   DTB 存在的意义是让**一个二进制**跑在**很多块板子**上 ——
   发行版必须这样做。但本课程里每个学生为**一个**配置编译
   **一份**内核, 并且明确知道自己给哪块板子编译。用运行期的数据
   结构表达一个编译期已知的事实, 是用复杂机制解决不存在的问题。

**代价**: 加新板子要重抄一遍地址。补偿措施是
[`docs/porting.md`](porting.md) —— 一份"抄哪些、从哪抄、抄完怎么
验证"的清单 —— 加上编译期断言 (例如
`harts.count() == ncpu`), 让抄错有很大概率在编译期被拦下。

### 3.3 唯一性用编译期机制, 多样性用 trait

这是贯穿全仓库的风格规则:

```text
  编译期唯一  ->  具体类型 / cfg / feature / 常量
  运行期多个  ->  trait + dyn
```

| 东西 | 唯一性 | 机制 | 理由 |
|---|---|---|---|
| 架构 | 编译期唯一 | `#[cfg(target_arch)]` | 见 3.1 |
| 平台 | 编译期唯一 | cargo feature | 一份内核只跑一台机器 |
| 定时器选型 | 编译期唯一 | `platform::TimerKind` 枚举 | 每个平台只有一种时间源 |
| UART | 一台机器一个 | 具体类型 `Uart16550` | 不需要多态 |
| PLIC | 一台机器一个 | 具体类型 `Plic` | 不需要多态 |
| **块设备** | **运行期可能有多个** | **`trait BlockDevice`** | virtio-blk 与 SD 卡可以同时存在 |

块设备是唯一使用 trait 的地方, 而且理由非常具体: 文件系统不应该
知道底下是 VirtIO 还是 SD 卡, 而"同时支持两种"在类型层面必须可
表达。用 `#[cfg]` 的话, 每加一种设备就要改所有调用点。

## 4. 与 C 版本的关键差异 (以及每一条的真实 bug)

本仓库是 `/home/wzh/oslab/ecnu-oslab-2026` (C 实现) 的 Rust 重写。
下面每一条都对应 C 版本里一个真实存在或注释里明确记录过的缺陷。

### 4.1 trapframe 的偏移不再手写 (用 `offset_of!`)

**C 版本**: `#define TF_RA (0*8)` 手写 34 个偏移, 加一个总大小的
`STATIC_ASSERT`。问题: 总大小对了, 但字段**顺序**错了查不出来 ——
而往中间插一个字段恰恰是最可能发生的情况。

**本仓库**: 汇编里的偏移由 `const { core::mem::offset_of!(TrapFrame, sepc) }`
提供。偏移量的**唯一来源是结构体定义本身**。结构体字段用数组
`[usize; 31]` 表示 x1..x31, 所以"插入一个字段导致后面全部错位"
在语法上不可能。

### 4.2 sret 之前的 sstatus 不再依赖"记得设置"

**C 版本**: 在 `trap_return_to_user` 里手工 `csrc sstatus, SPP` /
`csrs sstatus, SPIE`, 并在注释里用整页篇幅解释为什么必须这样做。
这是**对的**, 但它是"每一处返回都记得设置"的方案。

**本仓库**: 进入 trap 时把完整的 `sstatus` 存进 trapframe,
返回前由 `arch::trap::build_user_return_status()` /
`build_kernel_return_status()` **从 trapframe 重新构造**目标状态。
"忘记清 SPP"在语法上不可能 —— 因为返回路径根本不允许直接碰
`sstatus` (那条 `csrw sstatus` 在汇编里, 而且读的是 tf.status)。

同时保留了 C 版本那条硬性规则: 仍然**显式**设置 SPP=0/1 与
SPIE=1, 不依赖硬件的自动行为。

### 4.3 satp 的类型安全

**C 版本**: `arch_mmu_activate(uint64 root_pa)` 内部做
`root_pa >> PGSHIFT`。如果哪天有人在调用点自己写了 `write_satp`,
就会把物理地址当 PPN 写进去 —— 症状是**完全没有输出的静默卡死**。

**本仓库**: `PhysAddr` / `PhysPageNum` 是两个 newtype
(`#[repr(transparent)]`, 零开销)。`make_satp_sv39` 只接受
`PhysPageNum`, 而构造 `PhysPageNum` 的唯一途径是
`PhysAddr::page_num()` —— 它内部做移位。**这个 bug 在类型层面
无法表达。**

### 4.4 PLIC 的 claim/complete 用 RAII 配对

**C 版本**: `plic_claim()` 与 `plic_complete(irq)` 是两个函数,
靠调用者配对。忘记 complete 的症状是"中断只来一次就再也不来了"。

**本仓库**: `plic.claim(hartid)` 返回一个 `Claim` 守卫, 它的
`Drop` 里做 complete。而且 `Claim` **不是 `Copy`/`Clone`** ——
"一个中断被完成两次"这个错误同样无法表达。

### 4.5 hart 拓扑用区间, 不是 `hartid < ncpu`

**C 版本**的注释里明确记录了这个问题:

```text
  QEMU virt : 内核用 hart 0..1   -> 起点是 0
  VisionFive2: hart 0 是 S7 监控核, 内核只能用 1..4  -> 起点是 1
```

如果写 `hartid < ncpu`, VF2 上 `ncpu = 4` 会让**合法的 hart 4 被
误判为非法**, 症状是"4 核系统只起来 3 个核"且不报任何错。

**本仓库**: `platform::HartRange { min, max }` 提供 `contains()` /
`to_cpu_id()` / `to_hartid()`, 并且有编译期断言
`harts.count() == ncpu`。**所有**校验都走 `contains()`, 没有
任何调用点自己写比较表达式 —— 因为"自己写比较"正是这个 bug 的
来源。

### 4.6 构建期就校验链接地址

C 版本靠"`configs/*.mk` 的 `KERNEL_LOAD_ADDR` 与 `platform.h` 的
`PLAT_KERNEL_BASE` 保持一致"这句口头约定。两者不一致时症状是
静默跑飞。

**本仓库**有三道防线:

1. **链接期**: 链接脚本里的 `ASSERT(_entry == @KERNEL_BASE@)`;
2. **构建期**: `xtask` 用 `nm` 读出 `_entry` 的实际地址并与配置比对,
   不一致就报错并指出该去改哪个文件;
3. **启动期**: `config_selfcheck::check_link_address()` 比较
   `&_entry` 与 `platform.kernel_base` —— 这是唯一能发现
   "实际不是那里"的一道。

### 4.7 平台常量与构建配置的漂移被显式检测

"一台机器长什么样"记录在两个地方 (理由见 [`README.md`](../README.md)):
`configs/*.toml` (构建系统) 与 `platform/*.rs` (运行期代码)。
为了防止它们漂移, `config_selfcheck` 在启动的第一秒对比它们,
不一致就打印一句能读懂的话并停车。

原则是: **允许重复, 但强制它可验证。**

## 5. 启动流程 (以及为什么是这个顺序)

```text
  _entry (汇编, hal/src/arch/riscv64/boot.rs)
    ├─ 1. 校验 hartid 落在 platform.harts 区间内  -> 不在就停车
    ├─ 2. 建立本 hart 的内核栈 (区间索引 + 1)
    ├─ 3. hartid -> tp        (S-mode 读不到 mhartid)
    ├─ 4. 清 .bss             (objcopy 的裸二进制不含 .bss!)
    ├─ 5. 给栈打 canary
    ├─ 6. satp = 0
    ├─ 7. 关中断              (stvec 还没装)
    └─ 8. call kernel_entry

  kernel_entry (Rust, kernel/src/main.rs)
    ├─ SBI 控制台打印 "kernel entry reached"
    │     ^ 证明 CPU 活着, 且与 platform 常量无关
    ├─ 校验 hartid (平台层的 HartRange)
    ├─ 装"停车点" stvec      (把危险路径变成可观察的死循环)
    ├─ [启动核] config_selfcheck::run_all()
    ├─ [启动核] 打印启动横幅 (全部走 SBI)
    ├─ 初始化 UART 驱动 -> 注册为输出后端
    │     ^ 这一步之后所有输出走真实串口硬件
    ├─ 打印 "uart16550 ready @ 0x..." 
    │     ^ 证明 platform 常量正确 + 分频算对 + 硬件路径通
    ├─ 装真正的 trap_entry
    ├─ [启动核] 通过 SBI HSM 启动其他 hart
    ├─ 使能核间中断
    └─ 进 idle 循环 (带栈 canary 检查)
```

### 5.1 为什么前两行输出刻意走两个不同的后端

这是**分层验证**: 每一层用不依赖下一层的手段证明自己活着。

```text
  [oslab-rs] kernel entry reached (SBI console)   <- 只证明 CPU 活着
  [oslab-rs] uart16550 ready @ 0x10000000 ...     <- 证明板级常量正确
```

* 两行都有 -> 内核和平台都正常。
* 只有第一行 -> 问题一定在 platform 常量或 UART 驱动里, 不在
  更上层的代码里。

如果一开始就用 UART 打印, 那么"UART 地址写错"的表现是**什么都
没有** —— 包括那句本该告诉你"UART 地址可能错了"的话。

### 5.2 从核为什么需要自己的汇编入口

从核是被 SBI 在运行期叫起来的, 不是被固件在复位时跳进来的。
被唤醒时:

```text
  sp = 未定义   -> 不能执行任何 Rust 代码
  tp = 未定义   -> hartid() 返回垃圾
  a0 = 目标 hart 自己的 hartid  (SBI 规范保证)
  satp = 未定义
```

所以 `kernel/src/secondary.rs` 里也有一段 `global_asm!`, 用**与
启动核完全相同**的规则建栈 (同一个 `boot_stacks` 数组、同一套
索引), 并设置 `tp`。

> 这个坑在本仓库的第一版实现里真的踩到了: 启动核报告
> "2 now online", 但从核一行输出都没有 —— 因为它一进 Rust 就
> 从垃圾 `tp` 读 hartid, `cpu_id()` 返回 `None`, 立刻停车。

## 6. 关于"零依赖"

四个目标侧 crate 的依赖总数是 **0**。这不是洁癖:

1. 内核在裸机上跑, 没有 crates.io, 没有网络 —— 任何依赖都必须
   vendored;
2. 教学仓库里每一行第三方代码都是学生必须额外理解的负担;
3. 依赖越少, 构建越可复现。

`xtask` (宿主工具) 也是零依赖的, 包括它自己解析 TOML 与生成 FIT
镜像 —— 见 `xtask/src/config.rs` 与 `xtask/src/fit.rs` 顶部的说明。

## 7. 目录导航

```text
ecnu-oslab-2026-rs/
├── .cargo/config.toml          cargo xtask 别名
├── Cargo.toml                  workspace (目标侧与宿主侧分开)
├── rust-toolchain.toml         固定 1.97.1
├── configs/
│   ├── riscv64-qemu-virt.toml  QEMU virt + OpenSBI
│   └── riscv64-visionfive2.toml VF2 + U-Boot FIT
├── crates/
│   ├── uapi/                   系统调用 ABI (内核与用户程序共享)
│   ├── hal/                    arch facade + platform 常量
│   │   ├── build.rs            强制"恰好选中一个平台"
│   │   └── src/
│   │       ├── arch/riscv64/   CSR / Sv39 / trap / SBI / SMP / boot asm
│   │       └── platform/       qemu_virt.rs, visionfive2.rs
│   ├── drivers/                设备协议
│   │   └── src/{serial,irqchip,block,timer}/
│   └── kernel/                 OS 语义 (当前: 启动、自检、trap、打印)
│       ├── build.rs            生成链接脚本 + 平台常量清单
│       ├── linker/kernel.ld.in 链接脚本模板
│       └── src/{main,secondary,trap,console,panic,config_selfcheck}.rs
├── xtask/                      宿主构建工具 (零依赖)
│   └── src/{main,config,build,run,fit,architecture_rules}.rs
├── docs/
│   ├── architecture.md         本文件
│   ├── porting.md              如何加一块新板子
│   ├── board-deploy.md         VisionFive2 部署步骤
│   └── evidence/               实测输出 (启动日志等)
├── user/                       用户程序 (lab-4 之后使用)
└── tools/                      (预留)
```

## 8. 接下来 (留给后续 lab)

当前实现完成了**框架与可验证的启动路径**。下面是刻意留白的部分,
每一处都返回一个明确的 `Unsupported` / `NoSys` 而不是假装成功:

| 位置 | 内容 | 相关 lab |
|---|---|---|
| `kernel/src/trap.rs` 的 `SyscallFromUser` 分支 | 系统调用分发 | lab-4 |
| `kernel/src/trap.rs` 的缺页分支 | 按需分页 / COW | lab-3 / lab-6 |
| `drivers/src/block/virtio_blk.rs` 的 `read`/`write` | virtqueue | lab-7 |
| `drivers/src/block/sdhci.rs` 的 `init` 之后 | SD 卡识别流程 | lab-7 |
| `kernel/` 里还没有的东西 | 物理页分配器、进程、调度器、fs | lab-2..lab-9 |

返回明确的错误而不是静默的 `Ok(())` 是一条刻意的纪律:
后者会让调用者拿着未初始化的数据继续跑, 然后在完全无关的地方
崩溃。
