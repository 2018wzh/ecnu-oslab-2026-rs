# LAB-3: 中断异常初步

**前言**

前两个实验其实留了两个坑没有填:

- lab-1中实现了UART的输入输出函数, 但是printf只用到输出函数, 输入函数没有发挥作用

- lab-2中内核页表映射了PLIC, 还没有访问过它的能力

在本次实验, 我们会填上这两个小坑——为OS内核引入初级的"中断+异常"的识别和处理能力

## 1. 代码组织结构

```
crates/hal/src/arch/riscv64/
└── trap_entry.rs   用户态/内核态陷入时的寄存器保存与恢复 (global_asm) (NEW, 请完全理解这部分)

crates/drivers/src/irqchip/
├── mod.rs
└── plic.rs         SiFive PLIC 驱动 (两个平台复用, 只换地址) (TODO)

crates/kernel/src/
├── trap.rs         陷阱分发: 识别 scause, 判断中断/异常, 分发到具体处理 (TODO)
└── timer.rs        系统时钟: 计数、下一次中断、周期上报 (TODO)
```
**标记说明**

**NEW**: 新增源文件, 直接拷贝即可, 无需修改

**TODO**: 你需要实现新功能 / 你需要完善旧功能

## 2. RISC-V 的 trap 机制

中断、异常、陷入等概念在不同体系结构下(ARM, x86, MIPS...)定义有一些区别, 这里只讨论RISC-V的定义。

RISC-V用陷阱(trap)的概念统筹二者: 陷阱可以分为中断(interrupt)和异常(exception)两种类型。

一次 trap 的完整过程:

```
   发生中断/异常
        │
   硬件: sepc = 出错/被打断的 PC
         scause = 原因
         stval = 附加信息 (缺页地址等)
         sstatus.SPP = 陷入前的特权级
         跳到 stvec 指向的地址   ← 内核要提前设好
        │
   软件 (trap_entry.rs): 保存全部寄存器到 TrapFrame
        │
   kernel::trap::handle()  识别 scause 并分发
        │
   恢复寄存器, sret 返回
```

**区分中断与异常只看 scause 的最高位:**

```
   scause 最高位 = 1  -> 中断 (异步: 时钟、外部设备)
   scause 最高位 = 0  -> 异常 (同步: 缺页、非法指令、ecall)
   低若干位是具体编号
```

中断是"外面的世界在敲门", 异常是"你刚才那条指令出了问题"。两者的处理策略完全不同: 中断处理完可以原样返回, 异常往往意味着需要修正或终止当前执行流。

**共同点:**

- 中断和异常都是对正常执行流的一种打断, OS内核临时处理一个紧急的事情, 随后返回原来的执行流

- 中断和异常都涉及特权级的陷入和返回

**具体来说, RISC-V定义了以下中断和异常类型:**

- RISC-V为三个特权级 (U-mode, S-mode, M-mode) 分别定义了三种中断 (时钟中断, 软件中断, 外设中断)

- RISC-V定义了十几种异常类型 (包括内存访问越界, 非法指令, ecall等)

**关于CLINT和PLIC:**

- CLINT (core-local interruptor) 是每个CPU都有的机制, 负责接收**时钟中断和软件中断**

- PLIC (platform-level interrupt controller) 是所有CPU共享的机制, 负责接收**外设中断**

本次实验我们主要实现串口中断(一种外设中断)和时钟中断

## 3. 时钟中断

时钟是计算机的核心底层机制之一, 是机器指令有序执行的"心跳"或"节拍"。

RISC-V的定时器是**一次性**的: 到了时间触发一次, 然后就不管了。所以时钟中断处理函数里**必须重新设置下一次**:

```rust
timer::timer_reschedule();   /* 不重设 -> 第一次之后再也没有时钟中断 */
```

这是新手最常见的 bug: **第一次时钟中断正常, 之后就永远安静了。**

**为什么设置时钟要走 SBI:** `mtimecmp` 是 M-mode 的寄存器, S-mode 碰不到。所以内核通过 `SBI_TIME_SET_TIMER` 请固件代劳, 换来的是"同一份代码在所有 RISC-V 平台上都能设时钟"。

**只有时钟中断不需要任何外部事件就会到来。** UART 中断要有人敲键盘、磁盘中断要有 I/O 请求——而时钟中断每 0.1 秒必然发生。所以它是**唯一**能证明"中断系统确实在工作"的东西, 是本实验的验收标准。

## 4. PLIC: 外部中断控制器

PLIC 是 SiFive 的**平台级中断控制器**, 几乎所有 RISC-V SoC 都用它。两个平台的差异只有地址和中断号, **驱动源码完全复用**。

使能一个外部中断是"两级开关", 少任何一环中断就永远不来——而且不会有任何报错:

```
   ┌─ 设备自己产生中断
   │
   ├─ PLIC 使能了这一路     PLIC_SENABLE 的对应位
   │
   └─ CPU 的中断总开关打开   sstatus.SIE  +  sie.SEIE
```

PLIC 的 claim/complete 协议是**成对**的:

```
   loop {
        irq = plic.claim(hartid);        /* 认领一个待处理的中断 */
        if irq.is_none() { break }       /* 没有更多了 */
        处理 irq
        plic.complete(irq);              /* 告诉 PLIC: 处理完了 */
   }
```

**必须成对**。忘记 complete 的症状是"第一次中断来了, 之后再也没有了"——因为 PLIC 认为这个中断还在处理中, 不会再上报。

**中断号不能写死成 `1 << irq`:** VF2 的 UART0 中断号是 **32**。用 `1u32 << irq` 去算使能位时, RISC-V 的移位会**按位宽取模**——`1 << 32` 实际是 `1 << 0`, 于是你打开的是中断 0, 而不是 UART。正确做法是把"中断号"映射到"第几个寄存器、第几位":

```
   word = irq / 32
   bit  = irq % 32
   enable_regs[word] |= 1u32 << bit
```

这正是先把驱动写成"按中断号操作"、再由平台提供中断号的好处: **差异被收进常量, 而不是散在移位表达式里。**

## 5. 需要你完成的部分

本分支里下面这些**函数体是空的(`{ }`)**。分支之间只差**有哪些文件**。

| 文件 | 函数 | 属于 |
|---|---|---|
| `crates/kernel/src/console.rs` | `print_hex_bare` / `print_dec` | lab-1 |
| `crates/kernel/src/mm/pmem.rs` | `pmem_init` / `build_free_list` | lab-2 |
| `crates/kernel/src/mm/vm.rs` | `walk_create` / `map` / `kvm_init` | lab-2 |
| **`crates/kernel/src/trap.rs`** | **`init` / `handle_external`** | **lab-3** |
| **`crates/kernel/src/timer.rs`** | **`timer_create` / `timer_tick`** | **lab-3** |
| **`crates/drivers/src/irqchip/plic.rs`** | **`init_hart` / `claim`** | **lab-3** |

### 5.1 `trap::init` 做什么

把 `stvec` 指向 `hal::arch::trap_entry` 里的入口。在此之前 `stvec` 指向的是一个"停车点"——所以**在装好 stvec 之前不能开中断**。

### 5.2 一次时钟中断里的两件事

`trap_handler` 的时钟分支(已给出)做两件事, 顺序不能反:

```
   1. timer_reschedule()   立刻安排下一次 (不重设就再也没有时钟中断)
   2. timer_tick()         推进计数 (这是"中断真的在发生"的唯一证据)
```

你要实现的是这两件事的**内核侧接口**: `timer_create`(装第一次闹钟)与 `timer_tick`(记账)。

**为什么打印不在中断处理里做:** 中断上下文里做串口输出要跟其它 hart 抢锁、还可能被嵌套。所以"记账"和"显示"是分开的——计数在中断里推进, `idle_loop` 轮询它并把变化打印出来(见 §6.1)。这样一个写坏的处理函数最多让计数不对, 不可能把串口刷爆。

**为什么 `timer_tick` 的返回值要 "+1":** `fetch_add` 返回的是**加之前**的值。这个"差一"在计数器代码里错得特别多, 而症状(第一次 tick 打印出 0)很容易被误判成"打印时机太早"从而被忽略过去。

### 5.3 `plic::claim` 的返回值

`claim` 返回 `Option<Claim>`——一个 RAII 守卫。`Claim` 的 `Drop` 会自动 `complete`, 所以**拿着它处理完中断再让它离开作用域**, 就天然满足"成对"的要求。这是把协议约束编码进类型系统的一个例子。

## 6. 测试

### 6.1 QEMU

```bash
cargo xtask run --config riscv64-qemu-virt
```

实现正确时应当周期性看到时钟中断的计数在增长:

```
[oslab-rs] boot complete.
[oslab-rs] hart 0: 2 ticks (t=..., interval=1000000)
[oslab-rs] hart 0: 3 ticks (t=..., interval=1000000)
[oslab-rs] hart 0: 4 ticks (t=..., interval=1000000)
...
```

(`t=` 的具体数值每次运行都不同, 所以上面写成 `...`; 第一行的计数是 1 还是 2 取决于启动期间已经来过几次中断。关键是**计数在稳定增长**, 而 `t=` 每次大约加 `interval`。)

**这就是本阶段的验收标准**: 时钟中断真的在周期性触发。

这一行由 `idle_loop` 打印(`timer_print_ticks`), 三个数字可以互相校对:

| 字段 | 含义 | 怎么校对 |
|---|---|---|
| `N ticks` | 收到过几次时钟中断 | 每次中断 +1, 由 `timer_tick` 维护 |
| `t=` | 当时的绝对时钟 | 相邻两行之差应当约等于 `interval` |
| `interval=` | 平台的时钟间隔 | 必须与启动横幅里那一行一致 |

三点说明:

* **只有启动核会打印。** 每个 hart 都有自己的计数和闹钟, 全部打印会交错成"tick 5, tick 3, tick 6"这种无法解读的序列。
* **`t` 只差几百而不是 `interval`** -> 中断在风暴式触发(`deadline` 被设到了过去, 见 §3)。
* **`N ticks` 一直不动而 `t` 在涨** -> 中断来了但 `timer_tick` 没有推进计数。

### 6.2 出问题时怎么定位

| 现象 | 最可能的原因 |
|---|---|
| 只有 `boot complete`, 一行 tick 都没有 | `timer_create` 没装第一次闹钟; 或 `timer_tick` / `timer_reschedule` 是空的; 或没开中断总开关 |
| 第一次 tick 后安静 | 忘了重设下一次(`timer_reschedule` 只来一次) |
| `t=` 每次只差几百 | `deadline` 被设成了"过去"(把间隔当成了绝对时刻) |
| 装上 stvec 前就崩 | 中断没关, 而 stvec 还指向停车点 |
| 内核在第一次时钟中断后崩溃, 报 `store page fault` 且地址是个荒谬的负数 | trap 入口的**内核态**路径没有把内核 sp 从 `sscratch` 换回来(见 `hal` 的 `trap_entry`) |
| VF2 上 UART 中断不工作 | `1 << 32` 的移位陷阱 |

### 6.3 补充更多测试用例

助教给出的测试用例是远远不够的, 你需要补充更多测试用例以保证新增代码的正确性。

可以将你新增的测试用例和测试结果放在你的READM里面。

另外, 值得强调的一点是: 学会使用`panic`和`assert`做必要的检查。在出问题前输出有价值的错误信息, 比系统直接卡死或进入错误状态, 更容易Debug。

**尾声**

通过前三个实验, 我们搭建了OS内核的基础设施 (第一阶段)

- lab-1: 机器启动、标准输出、自旋锁

- lab-2: 物理内存、内核态虚拟内存

- lab-3: 中断和异常 (串口输入和时钟滴答)

一切的准备都是为了引出OS内核世界中最重要的概念--进程 (第二阶段)

- 进程需要基本的输入输出能力

- 进程需要自己的内存资源和虚拟地址空间

- 进程需要通过系统调用(一种异常)来获取OS内核服务

**新手村任务结束了, 准备接受更大的挑战吧......**

---

## 6. 进阶目标（可选）

下面三条**不属于基本验收**：默认流程与本分支 README 的期望输出都不依赖它们。
它们的作用是把这一阶段的内核"做完整一点"——每条都只用到**本章已经给出的东西**，
不碰后面阶段的文件。每条写了"为什么值得做""思路（只说做法，不写代码）""怎么算做到"。

> 动手前先建自己的分支（例如 `git checkout -b my-advanced`）。
> **别破坏默认输出**：进阶改动如果改变了默认运行结果，后面几章的对照实验就失效了；
> 需要改默认行为时，用一个新的 `configs/` 配置或一个运行期开关把它隔开。

---

### 6.1 内核 shell

**为什么值得做**：这是本课程第一次有"外部世界能打断内核"。shell 把这个能力变成
**不用改代码就能做的实验**：敲一个命令看 tick 涨了多少、中断来了几次。
更划算的是它能贯穿后面每一章——进程表、调度器、缓冲区缓存、目录树每章加一两条命令，
你的内核就有了自己的"观察窗口"。
还有一处必须自己补：`crates/drivers/src/serial/uart16550.rs` 给的是 `putc` / `getc` / `getc_blocking` 这类**单字符**接口，
`crates/kernel/src/console.rs` 只有输出侧（`print_dec` / `with_lock` 等），**没有任何行编辑或整行输入的 API**——从"收一个字符"到"读一行"这一层要你自己搭。

**思路**：
- 输入链路：用 `crates/drivers/src/irqchip/plic.rs` 的 `Plic::init_hart(hartid, &[irq])` 使能 UART 中断，
  在 `crates/kernel/src/trap.rs` 的 `handle_external` 里按 `Claim::irq()` 分发；UART 的分支只做一件事——用 `getc()` 取字符（它返回 `Option<u8>`，读空要立刻返回）放进一个静态环形缓冲。
- 行编辑在 `crates/kernel/src/console.rs` 里新增：提示符、回显、退格、回车成行；
  内核侧只需要一个非阻塞的"取一行"接口，编辑逻辑全部留在控制台层。
- 命令表用一个 `&'static [Cmd]`（名字 / 函数指针 / 一行帮助）即可，先做三个：帮助、tick 计数、中断计数。
- 两个容易踩的点：**不要在中断上下文里执行命令**（中断里只往缓冲区放字符，命令在主循环里跑）；**长命令要允许时钟中断**，否则你的 shell 会把整个系统卡住。
- 还有一个架构上的小难题：`oslab_drivers::serial::init_console()` 返回的 `Uart16550` 实例现在被 `crates/kernel/src/main.rs` 拿在手里，而中断处理里没有它。把"读一个字符"做成任何上下文都能调用的接口（`putc_raw` 那种关联函数是现成的范例），比到处传引用干净。

**怎么算做到**：三个命令都有输出；不敲键时 CPU 不空转；退格能改行；敲一个不存在的命令会提示而不是当掉。

**涉及**：`crates/kernel/src/console.rs`、`crates/kernel/src/trap.rs`、`crates/kernel/src/main.rs`、`crates/drivers/src/serial/uart16550.rs`、`crates/drivers/src/irqchip/plic.rs`　**难度**：★★☆

### 6.2 panic 与异常诊断升级

**为什么值得做**：后面每一章的 bug 都会先撞在这里。诊断信息的好坏直接决定调试速度——
本项目就是靠"异常码 + 故障地址 + 触发指令地址"这三件套定位了多个真实 bug；
也踩过"panic 的打印路径自己依赖一把已经坏掉的锁"这种坑。
Rust 版已经有一半材料：`crates/hal/src/arch/riscv64/trap.rs` 的 `FaultInfo` / `last_fault_info()` 会**一次取全**现场，
`TrapCause::name()` 已经能把原因变成字符串——这一条是把它们用足。

**思路**：
- 把内核对 trap 默认分支做成**现场报告**：异常类型、故障地址、触发指令地址（`TrapFrame::faulting_pc()`）、
  当前 hart/CPU 编号、当前进程（`current_pid()`），再把 `TrapFrame::reg(n)` 里关键的那几个寄存器打出来。
- 给 `TrapCause` 的每一类配一句"人话解释 + 下一步查什么"（例如写缺页 → 检查是不是往只读页写，
  或者页表里这一页忘了置写权限）；这份对照表写在 `crates/kernel/src/panic.rs` 或 `crates/kernel/src/trap.rs` 里都行。
- **输出路径不能依赖锁**：`crates/kernel/src/console.rs` 的 `with_lock` 在 panic 时可能死锁，
  要沿着 `oslab_hal::putchar::puts` 那条裸输出通道扩展（它本来就是为这种场合准备的）。
- 让 panic 之后其它 hart 也停下来：本仓库只有 `park_current_hart()`（停自己），没有"叫停别人"的机制——新增一个原子标志 + 从核在等待循环里检查，或者用 SBI 的 `hart_stop`；这是需要自己补的部分。
- 有余力再加一个"最近若干次调度/中断"的环形缓冲，崩溃时一并打印。

**怎么算做到**：故意空指针解引用 → 输出能指出异常类型、地址，并给出可执行的建议；
故意执行非法指令 → 输出里能看到指令字；另一个 CPU 正在打印时 panic 输出仍然可读。

**涉及**：`crates/kernel/src/panic.rs`、`crates/kernel/src/trap.rs`、`crates/kernel/src/console.rs`、`crates/hal/src/arch/riscv64/trap.rs`、`crates/hal/src/arch/riscv64/trap_entry.rs`　**难度**：★★☆

### 6.3 oneshot 定时器与动态间隔（tickless 的一半）

**为什么值得做**：现在的时钟是"固定间隔、无脑打断"，即使什么活都没有。
真实内核在没事时把下一次中断推远，甚至在空闲时干脆不设。这一章先做
"一次性定时器 + 由状态决定下一次时刻"，完整的 tickless 留到 lab-6（那时才有"超时队列"可以算出下一个事件）。

**思路**：
- 把"设置下一次中断"的**调用点**从"中断处理里固定间隔"改成"由当前状态决定"：
  有交互或有定时任务 → 短间隔；空闲 → 长间隔（或干脆不设，等别的事件）。
- Rust 版的接口形状要先看清楚：`crates/kernel/src/timer.rs` 的 `timer_reschedule` 调用的是
  `crates/hal/src/arch/riscv64/time.rs` 的 `set_next_deadline(interval)`——它收的是**间隔**，内部做 now+interval；
  真正收**绝对时刻**的是下面那层 `crates/hal/src/arch/riscv64/sbi.rs` 的 `set_timer`。
  要按"事件时刻"设闹钟，得自己加一层封装，并把"已经过去的时刻"夹到最小值——原样传下去就是中断风暴。
- 区分两个时间概念：**读时间**（`read_ticks()`，直接读 `time` CSR）与**设中断**（未来的某个绝对时刻）。
- 计数器语义会受影响：间隔变长后"每 tick 加一"会变粗。要么接受，要么把 `TICKS` 改成"毫秒时间戳"，并在注释里说清区别。
- 顺手把"等一下"这件事统一：`crates/hal/src/arch/riscv64/time.rs` 里的 `busy_wait_ticks` 是**忙等**（空转烧 CPU），别把它当成"睡若干 tick"来用；这条目标里的等待都应该由中断驱动。

**怎么算做到**：空闲时中断计数增长明显变慢，有活动时立刻恢复；
系统的等待语义仍然准确（该多久就多久）；系统不会因为"忘了设置下一次中断"而卡死。

**涉及**：`crates/kernel/src/timer.rs`、`crates/kernel/src/trap.rs`、`crates/hal/src/arch/riscv64/time.rs`、`crates/hal/src/arch/riscv64/sbi.rs`　**难度**：★★☆
