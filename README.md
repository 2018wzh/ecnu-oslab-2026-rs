# LAB-9: 文件系统 之 文件管理与全系统整合

**前言**

前面的实验逐步完成了内存、进程、文件系统和用户程序, 最后一个实验把它们接成一条完整的链:

路径 → inode → 缓冲区缓存 → 块设备, 以及磁盘上的 ELF 被真正加载

这个实验的任务不多, 但都落在"整合"上: 补文件描述符表、增加文件相关的系统调用、实现 exec 加载 ELF

## 1. 代码组织结构

```
crates/kernel/src/proc/
└── elf.rs      ELF 解析与加载 (TODO, 文件格式这一件事)

crates/kernel/src/fs/
├── file.rs     文件描述符表 (TODO, fd -> File -> inode)
└── dev.rs      设备文件 (把控制台之类的东西也纳入"文件"的统一视图)
```

**标记说明**

**TODO**: 你需要实现新功能 / 你需要完善旧功能

## 2. 文件描述符表 (crates/kernel/src/fs/file.rs)

本阶段需要在**crates/kernel/src/fs/file.rs**中实现文件描述符表, 核心是`File`结构: fd → File → inode

**读写位置 (`offset`) 属于 `File`, 不属于 inode** —— 这是整个设计的关键

**2.1 alloc / close 的要点**

- `alloc`: 找空位; 表满返回 `None`; 初始时 fd 0/1/2 已被占用
- `close`: 只关这一份; 对无效 fd 返回 `false`

**2.2 系统调用分支**

在`syscall.rs`的`dispatch`里补齐 `open` / `close` / `lseek` / `read` / `exec` 分支

注意两点:

1. **`write` 不能再用"fd 是不是 1 或 2"来判断目标**。用户 `close(1); open("log");` 之后 fd 1 指向的是文件, 判断数字的写法会把日志写到控制台上 (而且看起来"正常")。必须查 fd 表
2. **路径必须按 C 约定读到 `\0`**。Rust 的 `b"..."` 不补 `\0`, 忘了就会读到上限并返回 `BadArg`

**2.3 read_file_at**

在`mount.rs`实现`read_file_at` (带偏移的读), 注意循环里要处理"文件末尾": `off >= size` 时返回 0 字节 (EOF), 而不是错误

## 3. 加载并执行 ELF (crates/kernel/src/proc/elf.rs)

磁盘上的用户程序是以 ELF 格式存放的, 内核需要解析并加载它才能让用户程序真正跑起来

`elf.rs`回答的是"ELF 文件格式长什么样", `proc/user.rs`回答的是"怎么切换地址空间并进入用户态", 两者拆开, 出问题时能立刻知道该看哪个文件

本阶段需要在**crates/kernel/src/proc/elf.rs**实现`load_into`, 在**crates/kernel/src/proc/user.rs**实现`exec_current`

**3.1 load_into 的要点**

- 用户页表的用户部分是**空的**, 内核映射从 `kvm_global()` 复制
- 每段的 `vaddr` 必须落在 `USER_MIN..USER_MAX` 之内
- `memsz > filesz` 的部分要清零 (`.bss`)
- 返回值是入口地址

**3.2 exec_current 的要点**

- 先造好整张新地址空间 (页表 + 段 + 栈), **再**回收旧页面 —— 装载会失败, 而失败必须可恢复
- 回收时只扫 `USER_BASE..栈顶`, 不能扫整个低地址区 (那里有 MMIO)
- 重建 trapframe 时**先把 31 个寄存器清零**, 再设 `sepc` / `sp`
- 最后一步是 `activate_page_table`: 返回路径不会替你换页表
- 只有 `syscall.rs` 里 `dispatch` 的 `exec` 分支会被真正调用到 —— 它要先 `copy_str_from_user` 拷路径, 再把整个文件读进**静态**缓冲 (用户程序可达 70 KB, 内核栈只有 4 KiB), 最后交给 `exec_current`

## 4. 全系统整合

实现完成后, `init` 会 fork 出子进程, 子进程 `exec("/test_1")` 把映像换成磁盘上的 ELF (入口、段、权限全部来自那个文件), 跑完之后父进程 `wait` 回收它并拿到退出状态

前面所有阶段的工作 (页表、文件系统、块设备、trap、调度) 在这一刻被同时验证

`switch_to` 必须在 `context_switch` 之前 `activate_page_table(target.pgtbl)`: `fork` 出来的子进程页表是父进程的**拷贝**, 两边在同一个虚拟地址上的内容完全相同, 所以"切回父进程时忘了换页表"这个 bug 在 fork/wait 场景下完全看不出来 —— 直到某个进程用 `exec` 把自己的地址空间换成不一样的内容, 症状才突然出现

## 5. 测试

### 5.1 QEMU

```bash
cargo xtask run --config riscv64-qemu-virt
```

实现正确时应当看到 (关键部分):

```
[oslab-rs] 块设备自检: 按平台描述初始化
[oslab-rs]   设备: virtio-blk  容量: 2000 扇区
[oslab-rs]   读块 0 成功, 超级块魔数 = 0x10203040  <- 与 mkfs 写入的一致, 块设备通路正常
[oslab-rs] 从磁盘装载用户程序 (init)...
[oslab-rs]   找到 init: inode ..., ... 字节
[oslab-rs]   已读入 ... 字节
[oslab-rs]   ELF 入口 = 0x1000, 进程 pid=2 已就绪, 切换到用户态...
```

再看用户程序那一侧 —— init 会 fork 出子进程、`exec` 磁盘上的 `/test_1`、再 `wait` 回收它:

```
======== 测试开始 ========
---- test_1: 基本输出 ----
[test_1] 这一行走 stdout
[test_1] write 返回 29 (正确)
[test_1] 这一行走 stderr
[test_1] getpid() = 3
[test_1] OK

[oslab-rs] 进程 3 已退出, 状态码 0
-------- /test_1: 通过 --------
======== 测试结束 ========
```

磁盘上还放好了 `test_2` (文件读写)、`test_3` (fork/wait/退出状态)、`test_4` (exec 替换自身)。它们由 `cargo xtask disk` 打进同一个镜像, 需要时可以加进 `user/src/bin/init.rs` 的 `tests` 表里跑 —— 它们覆盖 open/read/lseek/exec 的更多组合

### 5.2 本阶段的验收点

除上面几行之外, 还要看到:

```
[init] 打开磁盘上的 /init (本程序自己) 并读前 4 字节:
[init] 读到 4 字节: 0x7f 0x45 0x4c 0x46
[init] 是 ELF 魔数 —— 文件系统通路正常。
```

这条路径串起了: `open` (路径解析) → fd 表 → 缓冲区缓存 → 块设备 → `read` (写回用户缓冲区) → `close`。它是"用户程序第一次通过系统调用访问磁盘"

再往下是 **exec** 这一段 —— 本阶段真正的"整合"验收点:

```
[init] exec("/no-such-file") 应当失败: 按预期失败, 本程序继续运行 (exec 失败可恢复)

[init] 调用 fork() + exec("/hello"):
[parent] fork 返回 pid=3
[child] 准备 exec("/hello") —— 下一行应当是 hello 的输出
[hello] 我是用 Rust 编译的用户程序。
[hello] 能看到这一行, 说明用户态是通的。

[oslab-rs] 进程 3 已退出, 状态码 0
[parent] wait 回收了 pid=3
```

两个关键点:

- `[hello]` 那两行不是 init 这个程序里的字符串 —— 它们是磁盘上 `/hello` 这个另一个可执行文件被 `exec` 装入之后打印的。看到它们, 说明"路径 -> inode -> 块设备 -> ELF 加载 -> 换地址空间 -> 换页表 -> sret"整条链是通的
- 有一条 `exec` 失败的验证排在前面。它证明 exec 失败时返回错误码、原程序继续运行 —— 这正是"exec 之前必须先 fork"的原因

### 5.3 出问题时怎么定位

| 现象 | 最可能的原因 |
|---|---|
| 没有"从磁盘装载用户程序"这一段 | 块设备自检就挂了; 先看"读块 0"那几行 |
| 找不到 `init` | `mount::lookup` 的名字比较有问题 (定长、非零结尾) |
| 前 4 字节不是 `7f 45 4c 46` | inode 读的偏移或块映射错了 |
| 找不到 fd | `open_stdio` 没被调用, 或 `alloc` 没考虑已占用的 0/1/2 |
| `exec` 之后跑的还是原程序 | 第 6 步 (激活页表) 漏了, 或 `p.pgtbl` 没换 |
| `exec` 之后执行的是毫不相干的代码 | 同上 —— 返回路径不会替你换页表 |
| `exec` 之后父进程 `wait` 回来就崩 | `switch_to` 没有激活**目标**进程的页表 (见下) |

---

## 4. 进阶目标（可选）

下面三条**不属于基本验收**：默认流程与本分支 README 的期望输出都不依赖它们。
它们的作用是把这一阶段的内核"做完整一点"——每条都只用到**本章已经给出的东西**，
不碰后面阶段的文件。每条写了"为什么值得做""思路（只说做法，不写代码）""怎么算做到"。

> 动手前先建自己的分支（例如 `git checkout -b my-advanced`）。
> **别破坏默认输出**：进阶改动如果改变了默认运行结果，后面几章的对照实验就失效了；
> 需要改默认行为时，用一个新的 `configs/` 配置或一个运行期开关把它隔开。

---

### 4.1 PIE 程序加载（位置无关 + 相对重定位）

**为什么值得做**：现在用户程序链接在固定地址（基址来自 `configs/arch/riscv64.toml` 的 `user_base`，
由 `xtask/src/user.rs` 生成 `user/arch/riscv64/user.ld.in` 的成品链接脚本），内核按 ELF 里写死的虚拟地址原样映射
（本章你要实现的 `crates/kernel/src/proc/elf.rs` 的 `load_into` 与 `crates/kernel/src/proc/user.rs` 的 `exec_current` 就是这条路径）。
PIE（位置无关可执行文件）允许加载到任意基址——它是地址空间随机化、共享库、"同一个程序被多次以不同基址加载"的前提，也是"链接器与加载器之间的契约"最集中的一块知识。

**思路**：
- 构建侧：用户程序改成按位置无关的方式布局（链接脚本给一个 0 基址的相对布局），并**保留重定位表**；
  注意 `xtask/src/user.rs` 现在对 ELF 做 `objcopy --strip-debug`（这是为了保住符号表），换成 PIE 之后要确认没有哪个选项把动态段或重定位表一起剥掉。
- 内核侧：对这类可执行文件先选一个加载基址（偏移量），所有段按"基址 + 段虚拟地址"映射，
  入口 = 基址 + 文件头里的入口（`ElfHeader` 里已经有 `entry`）。
- 然后处理重定位：从 `PT_DYNAMIC` 段里找到重定位表的位置/大小/表项大小，
  逐项处理"相对重定位"（把"基址 + 加数"写到指定位置）；RISC-V 上先只支持最常见的那一种就够。
- 两个坑：重定位表在**用户地址空间里**，必须经页表读写——`crates/kernel/src/mm/vm.rs` 的翻译接口与
  `crates/kernel/src/syscall.rs` 里 `copy_to_user` 那套做法都是现成的范例，**不能**直接解引用内核指针；重定位必须在"段都映射好之后、进入用户态之前"完成，`.bss` 部分也要覆盖到。
- 顺带一提：`user/src/runtime.rs` 里 `entry!` 宏生成的 `_user_start` **不收任何参数**。
  如果想让程序收到 `argc` / `argv`，得先扩展这个入口（以及 `crates/uapi/src/lib.rs` 里那条入口符号约定），那是另一件事，别和重定位混在一起做。

**怎么算做到**：同一个程序连续两次加载到**不同基址**都能跑通（打印自己的入口地址）；
故意不处理重定位表 → 程序在非零基址上崩（反证重定位是必需的）。

**涉及**：`crates/kernel/src/proc/elf.rs`、`crates/kernel/src/proc/user.rs`、`crates/kernel/src/syscall.rs`、`user/arch/riscv64/user.ld.in`、`xtask/src/user.rs`　**难度**：★★★

### 4.2 第二套文件系统（读得懂"别人的"镜像）

**为什么值得做**：现在只有一套自制格式，"文件系统"看起来只有一种做法。
去做一套**别人做的**镜像（例如 FAT 只读）会立刻暴露：哪些是我们的假设、哪些才是文件系统的普遍概念。
它也是上一章那条 VFS/挂载表进阶目标最自然的用途——如果加一套文件系统要改路径解析，说明抽象没抽干净。

**思路**：
- 只做**只读**：解析引导参数（每扇区字节数、簇大小、FAT 表位置、根目录位置），按 FAT 链遍历簇，读目录项（先支持短名——8 字符主名 + 3 字符扩展名——就够，长名是加分）。
- 新增的 `crates/kernel/src/fs/fat.rs` 实现文件系统接口（上一章那条进阶目标定义的 trait；没做的话，
  最低要求是让 `crates/kernel/src/fs/mount.rs` 的挂载路径变成"实现可替换"的样子），再挂到某个目录（例如 `/mnt/fat`），让路径解析自动支持它。
- 注意 FAT 是**小端**，而字段宽度不一（12/16/32 位）；`crates/kernel/src/fs/inode.rs` 里的
  `read_u16_le` / `read_u32_le` / `from_bytes` 是"逐字节组装、不直接读结构体"的现成范例，照这个思路自己写一份。FAT12 里"两个表项挤 3 个字节"是最容易读错的地方。
- 测试数据要真实：用宿主机的工具或下载一个 FAT 镜像，而不是自己写一个"像 FAT 的东西"——只有真的镜像才能证明你读对了；`xtask/src/mkfs.rs` 也可以加一条"造一个测试镜像"的路径。
- **不要做写支持**：写路径涉及两份 FAT 同步与目录项分配，工作量翻倍且容易把镜像写坏；
  写操作明确返回"只读文件系统"错误即可。
- 注意簇与扇区的换算，以及链上的环检测（访问计数上限或 visited 记录）。

**怎么算做到**：能列出真实 FAT 镜像里的文件并读出内容（与宿主机上看到的一致）；
写入被明确拒绝；损坏的 FAT 链（成环/越界）不会让内核死循环。

**涉及**：`crates/kernel/src/fs/mod.rs`、`crates/kernel/src/fs/mount.rs`、`crates/kernel/src/fs/inode.rs`、`xtask/src/mkfs.rs`　**难度**：★★★

### 4.3 procfs：把内核状态变成可读文件

**为什么值得做**：现在想看内核状态只能靠打印和 shell 命令；procfs 把这套**诊断能力**
变成用户程序也能读的接口（`cat /proc/ticks`）。它同时是"文件系统接口被非文件用途复用"
的经典案例——正好检验上一章那套抽象够不够通用（一个没有磁盘、内容按需生成的文件系统）。

**思路**：
- 做一套"内存文件系统"：每个文件的内容由一个**回调按需生成**，而不是静态数据：
  tick 数、进程表文本、内存统计（`pmem_stat()`）、挂载列表。在新增的 `crates/kernel/src/fs/proc.rs` 里实现文件系统接口。
- 挂到 `/proc`，用户程序 `open` / `read` / `close` 就能打印内核状态；`crates/kernel/src/fs/file.rs` 的 `FdTable` 与 `File` 一行都不用改，这正是抽象成立的证据。
- 三个语义要想清楚：**读是流式的**（要支持偏移量：重复读要么从头、要么接着上次——
  `File::offset` 已经给了你位置，`crates/kernel/src/fs/mount.rs` 的 `read_file_at` 是带偏移读的范例）；**长度**（能先算出来最好，算不出来就要定义"读到结尾"的行为）；**只读**（写操作返回明确错误）。
- 内容格式保持简单稳定（一行一条、字段固定），这样用户程序可以用它做断言——
  procfs 一旦稳定，它就是你自己的"验收工具"。
- 进程表文本从 `crates/kernel/src/proc/proc.rs` 导出，别在 procfs 里直接遍历 `PROCS` 那张私有表。

**怎么算做到**：用户程序连续两次读 `/proc/ticks`，值在增长；
读 `/proc/procs` 能列出当前所有进程（含 idle）；对 procfs 的写操作返回只读错误；
挂载列表里能看到 `/proc`。

**涉及**：`crates/kernel/src/fs/mod.rs`、`crates/kernel/src/fs/mount.rs`、`crates/kernel/src/fs/file.rs`、`crates/kernel/src/proc/proc.rs`　**难度**：★★☆
