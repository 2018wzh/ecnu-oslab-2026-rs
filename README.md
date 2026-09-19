# LAB-6: 单进程走向多进程——进程调度与生命周期

**前言**

经过前几个实验, 系统里第一次真正有了**多个**可运行实体

每个CPU有一个idle进程, 时钟中断会抢占当前进程

进程可以睡眠、被唤醒、退出并被回收

本实验主要围绕两个主题: 进程调度 + 生命周期

## 1. 代码组织结构

本阶段新增/完善的部分:

```
crates/kernel/src/
├── sched.rs       调度器: pick_next_and_switch, 每CPU的idle进程
└── proc/proc.rs   完善: sleep / wakeup (睡眠与唤醒)
```

本阶段不新增文件, 改动集中在 `sched.rs` 与 `proc/proc.rs`

本阶段需要实现的函数 (函数体是空的):

| 文件 | 函数 |
|---|---|
| `crates/kernel/src/proc/proc.rs` | `sleep` / `wakeup` / `proc_copy` |
| `crates/kernel/src/proc/user.rs` | `forkret` |
| `crates/kernel/src/mm/uvm.rs` | `copy_user_space` |
| `crates/kernel/src/sched.rs` | `idle_main` |
| `crates/kernel/src/sched.rs` | `pick_next_and_switch` |

## 2. idle进程

调度器的职责是"选一个可运行的进程运行"。如果一个都没有呢?

```
   所有进程都在等待 I/O / 时钟
        ↓
   调度器找不到任何可运行的进程
        ↓
   没有 idle: 只能 panic 或死循环 (而且死循环还占着 CPU)
```

所以每个CPU都要有一个**永远可运行**的idle进程:

```rust
loop { unsafe { arch::irq::wait_for_interrupt() } }   /* wfi: 不烧 CPU */
```

`wfi` 让CPU停下来直到有中断. 下一次时钟中断会把它唤醒, 调度器再重新挑一个进程

本阶段之前, 用户进程退出后内核就地空转——CPU满载却什么也没做. 有了idle, 退出后CPU会进入`wfi`, **真的停下来**. 这是"调度器在工作"最直观的证据, 也是本阶段的验收点

## 3. 进程状态机

```
   FREE ──► RUNNABLE ◄──────────┐
              │  ▲              │ wakeup
   被选中      │  │ 被抢占        │
              ▼  │              │
           RUNNING ────► SLEEPING
              │  sleep
              │ exit
              ▼
           ZOMBIE ──── 回收 ────► FREE
```

- **ZOMBIE 不能立刻消失**: 父进程还要通过 `wait` 拿到退出码
- **父进程先死**: 把子进程过继给别人, 否则它们永远等不到 `wait`
- **回收必须归还全部资源**: 内核栈、TrapFrame所在的页、整个用户地址空间

## 4. lost wakeup

```rust
unsafe fn sleep(chan: usize, lk: &SpinLock);
unsafe fn wakeup(chan: usize);
```

错误写法:

```rust
if condition_not_met { sleep(chan, lk); }   /* ✗ 判断与睡下之间有窗口 */
```

另一个CPU完全可能在这个窗口里改变条件并调用`wakeup`, 它发现进程还在运行, 于是**什么也不做**; 然后本进程才真正睡下去, 于是**永远醒不过来**

正确做法: 调用者持有保护条件的锁 `lk`, 在**持有锁的情况下**调用 `sleep`, 由 `sleep` 内部**原子地**完成"设置状态 + 释放锁":

```
   1. 设置 chan 与 state = SLEEPING
   2. 【此时才释放调用者的锁】      ← 顺序是关键
   3. 让出 CPU
   4. 被唤醒后: 清 chan, 重新获取 lk
```

第 2 步的顺序反了就会 lost wakeup

## 5. 具体任务

### 5.1 `pick_next_and_switch` 的策略

遍历进程表, 挑一个 `RUNNABLE` 的进程. **先挑"不是本CPU正在运行的"那个**, 否则你会"切换到自己", 那等于什么都没做

遍历不到任何可运行进程时, 切到本CPU的 **idle** 进程, 这就是"无路可退时的退路"

### 5.2 `sleep` 的顺序

```
   1. 设置 chan 与 state = SLEEPING
   2. 释放调用者的锁          ← 必须在 1 之后
   3. 让出 CPU
   4. 醒来后清 chan, 重新获取锁
```

### 5.3 `proc_copy`(fork) 要做的事

```
   1. 新建页表 (复制内核映射)
   2. 逐页真实拷贝用户内存     <- copy_user_space
   3. 复制 trapframe, 但:
        * 子进程 a0 = 0        ("一次调用, 两次返回且值不同")
        * sepc += 4            (跳过 ecall, 否则子进程会**无限 fork**)
   4. 继承 fd 表
   5. 伪造"第一次被调度"的现场: context.ra = forkret, sp = 栈顶 - TRAPFRAME_SIZE
```

**第 5 步里 sp 的取值有一个坑**: 本内核把trapframe放在内核栈顶的最后 `TRAPFRAME_SIZE` 字节里. 如果把sp设成 `kstack_top`, `forkret` 的函数序言一压栈就会**覆盖trapframe**, 而它下一步恰恰要用那个trapframe返回用户态. 症状是"子进程一进去就跑飞, sepc 变成 0"

所以起始sp要放在 `kstack_top - TRAPFRAME_SIZE`(trapframe之下)

### 5.4 `exit` 与 `wait` 的配合

```
   exit:  标 Zombie -> 唤醒父进程 -> 让出 CPU   (不能立刻回收!)
   wait:  在锁下检查有没有僵尸子进程
             有 -> 写回退出码, 回收槽位, 返回它的 pid
             没有但确实有子进程 -> 在锁下睡, 等 exit 唤醒后重试
             一个子进程都没有 -> 返回 NoEnt (这是调用者的逻辑错误)
```

**"检查"与"入睡"必须在同一把锁下**, 否则会 lost wakeup

### 5.5 回收资源时注意

Rust 版里 `Proc` 持有 `Option<&'static mut ...>` 之类的原始指针, **没有 Drop 会替你还内存**. 释放内核栈、用户页表这些事都要显式做

## 6. 测试

### 6.1 QEMU

```bash
cargo xtask run --config riscv64-qemu-virt
```

实现正确时应当看到(关键部分):

```
[init] 调用 fork():
[parent] fork 返回 pid=3
[parent] 调用 wait() ...
[child] 我是子进程 (fork 返回 0), 准备 exit(7)
[oslab-rs] 进程 3 已退出, 状态码 7
[parent] wait 回收了 pid=3
[oslab-rs] 进程 2 已退出, 状态码 0
```

**三个验收点:**

1. 用户进程的pid是 **2**(pid 1 是启动流程变成的进程)
2. 退出时打印"状态码 0"(`proc_exit` 走到位了)
3. 调度器切到 idle, `switch` 真的在工作

**尾声**

本实验围绕进程管理的主题, 从一到多构建了进程管理模块

进程能变多了, 但它们除了打印什么都做不了: 没有块设备、没有文件系统

下一阶段开始引入磁盘管理——块设备驱动、缓冲区缓存、位图分配

---

## 6. 进阶目标（可选）

下面三条**不属于基本验收**：默认流程与本分支 README 的期望输出都不依赖它们。
它们的作用是把这一阶段的内核"做完整一点"——每条都只用到**本章已经给出的东西**，
不碰后面阶段的文件。每条写了"为什么值得做""思路（只说做法，不写代码）""怎么算做到"。

> 动手前先建自己的分支（例如 `git checkout -b my-advanced`）。
> **别破坏默认输出**：进阶改动如果改变了默认运行结果，后面几章的对照实验就失效了；
> 需要改默认行为时，用一个新的 `configs/` 配置或一个运行期开关把它隔开。

---

### 6.1 睡眠与超时队列

**为什么值得做**：本章刚刚实现的 `sleep(chan, lk)` / `wakeup(chan)` 只能等别人叫醒，
没有任何"睡一段时间"的能力。Rust 版里连"等若干 tick"这个能力都不存在——
`crates/hal/src/arch/riscv64/time.rs` 的 `busy_wait_ticks` 是**忙等**（关着中断空转烧 CPU），不是睡眠，谁把它当超时用，谁的进程就会占着 CPU 不放手。
真实内核用一个按到期时刻排序的队列管理所有定时等待，它同时是 6.3 tickless 的前置。

**思路**：
- 在 `crates/kernel/src/timer.rs` 里新增"睡 n 个 tick"（这是**需要新增**的接口，仓库里没有）：
  到期时刻 = `timer_get_ticks() + n`，然后睡在一个专用通道上。`sleep` 收的 `chan` 就是一个 `usize`，用超时队列结构体自己的地址当通道最省事。
- 维护一张"到期时刻 → 进程"的有序表（升序链表或有数组足够，小根堆是加分）；
  内核里还没有堆，所以表长固定，满了要有明确行为（拒绝 vs 报错）。
- 时钟中断里检查队首是否到期并 `wakeup`；被别的事件提前唤醒的进程要从队列里**摘掉**，
  否则会留下"过期的等待者"（症状是莫名被唤醒一次，而且通道号可能已经指向别的用途）。
- 说清时间语义：队列里存的是**绝对时刻**；而 `timer_reschedule` 用的
  `crates/hal/src/arch/riscv64/time.rs` 的 `set_next_deadline` 收的是**间隔**（内部做 now+interval），
  真正收绝对时刻的是 `crates/hal/src/arch/riscv64/sbi.rs` 的 `set_timer`。要把"队首到期时刻"设成下一次闹钟，得自己加一层封装，并把已经过去的时刻夹到最小值。
- 被终止或退出的进程要能从队列里清理干净（想想 6.3 里"队首指向一个僵尸进程"会怎样）。

**怎么算做到**：多个进程按到期顺序被唤醒（打印各自睡了多少 tick）；
提前唤醒之后队列长度回到 0；唤醒时刻误差在一个 tick 内。

**涉及**：`crates/kernel/src/timer.rs`、`crates/kernel/src/proc/proc.rs`、`crates/kernel/src/trap.rs`、`crates/hal/src/arch/riscv64/time.rs`　**难度**：★★☆

### 6.2 可切换的调度策略

**为什么值得做**：这一章的设计要点是"机制与策略分离"，而 `crates/kernel/src/sched.rs` 的 `pick_next_and_switch`
就是那个策略点（现在轮转逻辑和切换写在同一个函数里，真正做切换的 `switch_to` 是它的下半段）。
"可切换策略"是检验这句话是否真的成立的最直接方式——如果换策略要动 `switch_to`，说明分离没做到。
它也很容易产出一份有数据的实验报告。

**思路**：
- 定义一个很小的策略接口：一个"选下一个"的函数 + 一个"时钟到来时"的回调（用于时间片计数、优先级老化等）。
  Rust 里两种写法都行：`trait SchedPolicy` + `&'static dyn SchedPolicy`，或者一个函数指针结构体。
  本仓库的风格是"编译期唯一的用常量/cfg，运行期真的会换才用 trait/dyn"——策略是运行期可换的，所以后者是合理选择；
  但请把选择理由写进注释，别照抄某一种。再写 2–3 个实现：简单轮转、时间片可调、优先级，乃至"空闲核去偷一个就绪进程"。
- 用已经做过的内核 shell 在运行期切换，并打印每个进程的"被调度次数/等待时长"做对照；
  `LAST_PICKED` 与 `IDLE_PROC` 这两张 per-hart 表就是现成的线索。
- 边界想清楚：状态迁移（`ProcState` 的就绪/运行/睡眠）属于**机制**，不能跟着策略走；
  时间片计数放"机制侧"还是"策略侧"要先决定（建议回调只做计数，选择只做选择）。
- 多核别忘了：两个 hart 同时"选"必须互斥。框架里现成的只有 `INITED` 这个 `AtomicUsize`
  （见 `crates/kernel/src/sched.rs`），"选谁 + 改状态"这两步并没有锁保护 —— 正好用
  `crates/kernel/src/sync.rs` 的自旋锁补一把，并顺手想清楚它与进程表锁的锁序；
  `pick_next_and_switch` 被调用时可能已经持有别的锁（`sleep` 的 `lk` 就是一把），别在这里引入新的死锁。

**怎么算做到**：切换策略只用改策略表、不动 `switch_to`；两种策略下每个可用进程都能被跑到（无饿死）；对照数据能解释差异。

**涉及**：`crates/kernel/src/sched.rs`、`crates/kernel/src/proc/proc.rs`、`crates/kernel/src/timer.rs`、`crates/kernel/src/sync.rs`　**难度**：★★★

### 6.3 完整 tickless：让下一次中断跟着事件走

**为什么值得做**：上一章做到了"动态间隔"，但间隔还是拍脑袋定的。
真正的 tickless 是"下一次中断 = 最近一个到期事件"（超时队列的队首、当前进程时间片到期），
空闲且无事件时干脆不设。它把"时钟"从"心跳"变成"闹钟"，也是移动设备省电的根本原因。

**思路**：
- 在一个函数里算出"下一个事件时刻"：超时队列队首到期时刻、当前进程时间片到期时刻，取最小。
  放在 `crates/kernel/src/timer.rs` 里最自然（队列在那里），调度器调用它。
- 在几个关键位置调用它：调度切换之后、进程睡下去之后、时钟中断返回前。
- 空闲且队列为空时不再设置中断（等外部中断唤醒），需要时再设。
  Rust 版的接口形状正好合用：`set_next_deadline` 收的是**间隔**，所以把"下一个绝对时刻"减去"现在"即可；差值为 0 或为负时夹到最小值——把过去的时刻原样传下去就是中断风暴。
- 时间源精度决定最小间隔；如果 `crates/kernel/src/timer.rs` 的 `TICKS` 与"真实时间"绑得太死，考虑改成时间戳（`read_ticks()`）。
- 最经典的坑：**忘了设置下一次中断 → 系统永远不再被唤醒**。调试期用一个 shell 命令打印
  "现在时刻 / 下次中断时刻 / 队首到期时刻"，一眼就能看出漏设。

**怎么算做到**：空闲（无进程可跑、无定时等待）时中断计数停止增长；
有定时等待时唤醒误差在一个 tick 内；系统长时间运行不会卡死。

**涉及**：`crates/kernel/src/sched.rs`、`crates/kernel/src/timer.rs`、`crates/kernel/src/trap.rs`、`crates/hal/src/arch/riscv64/time.rs`　**难度**：★★★
