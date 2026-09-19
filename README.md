# LAB-4: 第一个用户进程的诞生

## 1. 代码组织结构 (本阶段新增的部分)

```
crates/kernel/src/
├── proc/
│   ├── mod.rs      进程控制块、进程表、公开接口
│   ├── context.rs  内核上下文 (callee-saved 寄存器)
│   ├── switch.rs   上下文切换 (global_asm)
│   ├── user.rs     进入用户态 + 用户镜像加载
│   └── ...         进程相关的一切
├── sync.rs         自旋锁 (供进程表与调度使用)
└── syscall.rs      系统调用分发 (从 trapframe 取参数)  (TODO)

crates/kernel/src/main.rs 里的 `user_images` 模块把用户程序
(编译产物) 以字节数组的形式嵌进内核镜像。
```

## 2. 需要你完成的部分

本阶段要做的是让内核"生"出第一个用户进程: 准备它的地址空间, 切换进用户态, 并
正确响应它发出的系统调用。`trap.rs` 已经完整处理了"从内核态陷入"的情况, 你的
工作是在 `handle` 里补上"从用户态陷入"的分支, 结构几乎一样, 区别只有三点:

1. 保存/恢复要经过 `TrapFrame` (用户寄存器必须一个不少)
2. 要处理 `ecall from U` (系统调用), 并且 `sepc += 4`
3. 用户态缺页是**用户程序的错**, 应当终止它, 而不是让内核 panic

需要实现的函数有四处。

### 2.1 proc_make_user: 创建第一个用户进程

`crates/kernel/src/mm/uvm.rs` 里的 `proc_make_user` 按顺序完成:

1. 分配一个进程槽位与内核栈
2. 在内核栈顶端放 TrapFrame
3. 把用户镜像按页映射到用户地址空间 (必须带 U 权限)
4. 分配并映射用户栈
5. 设置 TrapFrame 的 `sepc` (入口) 与 `sp` (栈顶)

**少了 `U` 权限位的症状**: 进入用户态后立刻取指缺页。

### 2.2 enter_user: 进入用户态

`crates/kernel/src/proc/user.rs` 里的 `enter_user` 让进程进入用户态:

1. 设为当前进程 (否则系统调用里 `myproc()` 拿到的是错的)
2. 切换页表
3. 把内核栈顶写进 `sscratch` (下次陷入时靠它换栈)
4. 走"从 trap 返回"的路径 —— 不返回

### 2.3 dispatch: 系统调用分发

`crates/kernel/src/syscall.rs` 里的 `dispatch` 从 trapframe 取系统调用号和
参数并分发。两件容易忽略的事:

1. **推进返回地址** (位置在 `trap.rs`, 已给出): 系统调用陷入时 `sepc` 指向
   `ecall` 那条指令本身, 不推进就会无限重复。
2. **错误码区分"没实现"与"调用号不存在"**:

   | 情况 | 返回 |
   |---|---|
   | 号在 ABI 里有定义但本阶段没实现 (目前只有 `mmap = 9`) | `SysError::NoSys` (`-38`) |
   | 号在 ABI 里根本不存在 | `SysError::BadArg` (`-1`) |

   两者都是负数, 但排查方向相反: `-1` 要去查自己的参数/调用号, `-38` 说明内核
   确实还没实现这个功能。`docs/abi-spec.md` 有同一张表。

### 2.4 copy_from_user: 把用户指针变成可信的数据

后续阶段 (lab-5 起) 要给用户传入的参数地址做校验时 (例如 `copy_str_from_user`
读路径、lab-9 的 `write` 读用户缓冲区), `copy_from_user` 是"读用户内存"的唯一
入口, 它必须做两个检查:

| 检查 | 不做的后果 |
|---|---|
| 地址落在**用户区** (低于 `platform().kernel_base`) | 用户传一个内核地址, 内核就替它把内核内存读出来 (提权) |
| 这一页**确实映射了**, 而且要用**当前进程**的页表翻译 | 直接解引用未映射地址 → 内核缺页 → 用户把内核搞崩 |

```text
   unsafe fn copy_from_user(va: usize) -> Option<u8> {
       if va >= oslab_hal::arch::cpu::platform().kernel_base { return None; }
       let pa = user_translate(va)?;          /* 走当前进程的页表 */
       Some(unsafe { *(pa as *const u8) })
   }
```

两个容易忽略的点:

* **必须用 `user_translate` (当前进程的页表), 不能用内核全局页表
  `kvm_translate`**: 同一个虚拟地址在不同进程里指向不同的物理页。
* **逐字节就够了**: 一次 `write` 的字符串很短, 而且逐字节天然处理跨页边界。

## 3. 进入用户态

RISC-V 没有"跳到 U-mode"的指令, 只有 `sret` (从 trap 返回)。做法是伪造一个
TrapFrame: 把 `sepc` 设为程序入口, 把 `sp` 设为用户栈顶, 然后走一遍
"从 trap 返回"的路径。第一次进入用户态和系统调用返回用户态走的是同一段代码。

`sret` 之前必须显式设置 `sstatus`:

```
   SPP  = 0     返回 U-mode (硬件留下的值可能是 1!)
   SPIE = 1     返回后打开中断
```

少了这两行, 最常见的症状是**内核静默卡死**: CPU 以为要返回 S-mode, 继续用
内核页表执行用户代码。

## 4. 系统调用: 一组约定

用户程序要输出一段文字, 它不能直接写 UART 寄存器, 必须请求内核替它做。本阶段
的用户进程只发一个最简单的请求, 内核收到后打印固定字符串 `proczero: hello world`。
系统调用是一组约定: 用户程序通过寄存器传入系统调用号, 内核根据调用号分发到
对应的实现, 提供系统服务后返回处理结果, 用户程序和内核通过 trap 机制通信, 实现
跨特权级的"函数调用"。

几个关键点:

- 调用号与参数必须**从 TrapFrame 取** —— 那是用户寄存器的快照, 内核拿不到
  "用户此刻的寄存器"。
- 返回值必须**写回 TrapFrame 的 a0**, 否则 `sret` 恢复寄存器时会用旧值覆盖掉
  返回值。
- `sepc` 必须 **+4**: `ecall` 是一条指令, 异常返回时 `sepc` 指向它自己, 不 +4
  就会无限重复执行同一条 `ecall`。

## 5. 测试

### 5.1 QEMU

```bash
cargo xtask run --config riscv64-qemu-virt
```

实现正确时, 会在串口上看到用户程序打印的这段:

```
proczero: hello world
proczero: hello world
```

**这几行文字来自用户态** —— 这是本阶段唯一的验收标准。

`write` / fd 表 / `copy_from_user` / `getpid` / fork / open 属于后续阶段
(lab-5 / lab-6 / lab-9); 本阶段只证明用户态通路最基础的那一段。后续阶段的内容,
本阶段不应该成功, 程序里已经对每一种失败都做了打印 (这样"哪一步还没铺路"一眼
可见)。

---

## 6. 进阶目标（可选）

下面三条**不属于基本验收**：默认流程与本分支 README 的期望输出都不依赖它们。
它们的作用是把这一阶段的内核"做完整一点"——每条都只用到**本章已经给出的东西**，
不碰后面阶段的文件。每条写了"为什么值得做""思路（只说做法，不写代码）""怎么算做到"。

> 动手前先建自己的分支（例如 `git checkout -b my-advanced`）。
> **别破坏默认输出**：进阶改动如果改变了默认运行结果，后面几章的对照实验就失效了；
> 需要改默认行为时，用一个新的 `configs/` 配置或一个运行期开关把它隔开。

---

### 6.1 从"扁平映像"到 ELF 映像

**为什么值得做**：现在第一个用户程序是 `objcopy -O binary` 出来的**扁平映像**：
`crates/kernel/build.rs` 把构建出来的用户程序扁平映像用 `include_bytes!` 嵌进内核，
`crates/kernel/src/mm/uvm.rs` 的 `proc_make_user` 把它从 `USER_BASE` 起逐字节装入就跳过去，没有"段"的概念。
真实的用户程序是 ELF——内核必须按 program header 逐段映射、按段设权限、把 `memsz > filesz` 的部分清零。从磁盘加载 ELF 是后面某一章的内容；这一章可以先把"解析 ELF"单独练一遍，数据来源是**内嵌的数组**（不需要文件系统）。

**思路**：
- 构建侧：`xtask/src/user.rs` 其实已经同时产出了 `<程序名>.stripped.elf`（它刻意用 `objcopy --strip-debug` 而不是 `--strip-all`，就是为了保住符号与段），改用这个产物即可；`crates/kernel/build.rs` 的嵌入清单跟着换。
- 内核侧：新增的 `crates/kernel/src/proc/elf.rs`，写"内存里 ELF 的解析与校验"：魔数/类别/字节序/机器码、逐个 program header、
  虚拟地址必须落在用户地址范围内、按段标志给出读/写/执行权限、`memsz > filesz` 的部分清零
  （Rust 里没有 `memset`，用 `core::ptr::write_bytes` 或一个循环）。
- 用 `crates/kernel/src/mm/pmem.rs` 的 `pmem_alloc(Pool::User)` 与 `crates/kernel/src/mm/vm.rs` 的 `map` 把每个段装进去；
  入口由 ELF 头里的 `entry` 给出，不再假设"入口 = USER_BASE"——这正是 `user/arch/riscv64/user.ld.in` 顶部那段
  "文件偏移必须等于虚拟地址偏移"的约束被解除的地方。
- **所有偏移与长度都来自外来数据**，每一处都要边界检查（这是从磁盘加载 ELF 的预演）。
- 别忘了 `crates/kernel/src/proc/user.rs` 里的 `.bss` 清零逻辑：ELF 路径下清零范围来自 `memsz`，不再是链接脚本符号。

**怎么算做到**：换成 ELF 映像后 initcode 照常跑通；故意改坏一个段的虚拟地址或大小 → 被明确拒绝并给出原因，而不是跑飞。

**涉及**：`crates/kernel/src/proc/user.rs`、`crates/kernel/src/proc/mod.rs`、`crates/kernel/build.rs`、`xtask/src/user.rs`、`user/arch/riscv64/user.ld.in`　**难度**：★★★

### 6.2 第一个内核执行流（内核线程的雏形）

**为什么值得做**："进程"现在只属于用户态，但真实内核里有只在内核里跑的执行流
（idle、后台刷盘、解压 initramfs）。这一章的起点其实不低：
`crates/kernel/src/proc/context.rs` 的 `Context::new`、`crates/kernel/src/proc/switch.rs` 的 `switch`、
`crates/kernel/src/sched.rs` 的轮转都已经给出——但 `crates/kernel/src/main.rs` 是**直接** `enter_user()` 进用户态的，
这台机器还没有为"一条只在内核里跑的执行流"转过一次。把它做出来，下一章的"内核线程"就只差"能被调度"这一步。

**思路**：
- 用 `crates/kernel/src/proc/proc.rs` 的 `proc_alloc()` 拿进程槽与内核栈（它会把 `kstack_top` 填好），
  但**不要**填 trapframe：这条执行流的入口是一个内核函数，不是从陷阱返回用户态。
- `Context::new(入口地址, kstack_top)` 已经把"伪造一个第一次被调度的现场"这件事做好了；
  用 `crates/kernel/src/proc/switch.rs` 的 `switch` 直接切过去，不经过 `pick_next_and_switch`。
- 先让它跑完打印一行就停下（等待中断时用 `wfi`；仓库里现在是各处直接写内联汇编，顺手封装成一个统一的等待函数是个不错的附带收益），
  `crates/kernel/src/main.rs` 里顺序调用一次即可。
- 提前想清楚下一章会用到的两件事：上下文保存在 `Proc::context` 里、被切换回来时从 `Context::ra` 继续；
  另外 `TrapFrame` 上那个"设置内核栈顶"的接口是给**用户态陷入**用的，内核执行流之间切换不经过它。
- 注意这条执行流和 idle 进程的区别：idle 永远可运行、优先级最低，而它应该能跑完就结束——结束时的清理路径要自己写。

**怎么算做到**：能打印出这条执行流自己的内核栈地址区间；它执行期间 `current_pid()` 指向它自己；
它不返回用户态，也不踩启动流程的栈。

**涉及**：`crates/kernel/src/proc/proc.rs`、`crates/kernel/src/proc/context.rs`、`crates/kernel/src/proc/switch.rs`、`crates/kernel/src/main.rs`　**难度**：★★☆

### 6.3 HHDM：把物理内存线性映射到高半区

**为什么值得做**：现在内核是**恒等映射**——虚拟地址等于物理地址（见 `crates/kernel/src/mm/vm.rs` 里关于恒等映射的那段说明），
所以"物理地址 0x80200123"与"虚拟地址 0x80200123"在代码里长得一模一样，指针到底是哪种地址只能靠注释和记忆。
真实内核把全部物理内存线性映射到一个高半区（HHDM），于是"物理页"永远通过"物理地址 + 偏移"访问，谁是物理地址一眼可辨。
Rust 版还多一层收益：`crates/hal/src/arch/riscv64/mm.rs` 的 `PhysAddr` / `VirtAddr` / `PhysPageNum` 三个 newtype
本来就是为这件事准备的——把转换写进类型，编译器能替你抓住大部分"忘了转换"的错误。

**思路**：
- 链接脚本 `crates/kernel/linker/smode.ld.in` 改成把内核放到高半区。注意它的链接地址来自 `configs/*.toml` 的 `kernel_load_addr`
  并由 `crates/kernel/build.rs` 注入——先把"加载地址"与"链接地址"是两个概念这件事想清楚，再动脚本。
- 入口先建**两份映射**（一份恒等、一份高半区），跳到高地址之后再撤掉恒等映射——这是经典的 higher-half 引导流程；
  `crates/hal/src/arch/riscv64/boot.rs` 的汇编里现在就用 `la sp, boot_stacks` 建栈，栈指针也得跟着搬到高半区。
- 定义偏移常量与两个转换函数（物理↔虚拟），并规定：凡是"物理页号/物理地址"，使用前必须转换。
- 页表接口的语义要写清楚：页表里存的是**物理**地址，翻译接口返回的也是物理地址——高半区之后这条约定更容易被误用。
- 这是本章最伤筋动骨的一条：会影响后面每一处指针运算。建议分三步走、每步都能跑：① 内核镜像上高半区；② 全内存线性映射；③ 把分配器、设备映射、页表代码改成用转换函数。

**怎么算做到**：内核跑在高半区（打印一个内核函数地址，高位全 1）；物理↔虚拟转换往返一致；设备仍可访问（串口输出正常）；`git grep` 里不再有"裸物理地址直接当指针用"的地方（引导早期除外）。

**涉及**：`crates/kernel/linker/smode.ld.in`、`crates/hal/src/arch/riscv64/boot.rs`、`crates/kernel/src/mm/vm.rs`、`crates/kernel/src/mm/pmem.rs`、`crates/hal/src/arch/riscv64/mm.rs`　**难度**：★★★
