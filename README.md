# LAB-8: 文件系统 之 数据组织与层次结构

**前言**

在lab-7中我们实现了block-level的磁盘读写能力 (bio, bitmap)

在此基础上, 本次实验进一步考虑以下两个问题:

1. 一个文件可能远大于1个block, 如何组织和管理这些数据块? (inode)

2. 如何用人类更好理解的层次结构来组织和索引海量文件? (dentry, 目录)

## 1. 代码组织结构

```
ecnu-oslab-2026-rs
├── Cargo.toml       工作空间定义
├── configs          板级配置文件 (qemu, visionfive2)
├── crates           各 crate 源码
│   ├── kernel       内核 crate
│   │   └── src
│   │       ├── console.rs  控制台输出
│   │       ├── fs          文件系统模块
│   │       │   ├── bio.rs      (lab-7) 缓冲区缓存
│   │       │   ├── bitmap.rs   (lab-7) 块位图管理
│   │       │   ├── inode.rs    (TODO, 核心工作) 磁盘 <-> 内存 inode
│   │       │   ├── dir.rs      (TODO, 核心工作) 目录项与路径解析
│   │       │   └── mount.rs    挂载与初始化 (CHANGE)
│   │       ├── mm       内存模块
│   │       ├── proc     进程模块
│   │       ├── sched    调度模块
│   │       ├── syscall  系统调用模块
│   │       ├── trap     陷阱模块
│   │       └── timer.rs 定时器
│   ├── drivers     设备驱动
│   ├── hal         硬件抽象层
│   └── uapi        内核对外接口
├── user        用户程序
└── xtask       构建脚本 (cargo xtask ...)
```

**标记说明**

**CHANGE**: 旧的源文件发生了更新, 直接拷贝即可, 无需修改

**TODO**: 你需要实现新功能 / 你需要完善旧功能

## 2. inode: 怎么装下比一个块大的文件

一个块 512 字节, 一个文件可能有 100 KB 甚至更大。磁盘上的 inode
负责记录一个文件的所有数据块位置:

```rust
pub const NDIRECT: usize = 12;
pub const NINDIRECT: usize = BLOCK_SIZE / core::mem::size_of::<u32>();  /* 128 */

/* 磁盘上的 inode */
pub struct DiskInode {
    pub nlink: u16,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
    ...
}
```

**addrs[] 是索引表**, 分成两部分:

- `addrs[0..11]`: 12 个直接块 -> 12 × 512 = 6 KB

- `addrs[12]`: 一级间接块 -> 128 × 512 = 64 KB

一级间接块里存的是**块号数组**, 不是数据。真实文件系统会用二级、
三级间接块 (ext2) 或 extent/B 树 (ext4), 本次实验实现到一级间接块为止。

### 2.1 逻辑块号到物理块号

读/写文件数据时, inode 的**索引层判定**把**逻辑块号**转译为**物理块号**。
这个分层边界由 `DiskInode::block_level(n)` 集中完成: 返回 `(层级, 下标)`,
边界条件只写一次、只测一次。

```
   bn < 12           -> 直接块:  addrs[bn]
   bn < 12 + 128     -> 间接块:  读 addrs[12], 取其中的 [bn - 12]
   bn 超出           -> 需要分配新块
```

三个容易出错的地方:

1. **边界**: `bn == 12` 应该走间接块的第一项, 而不是 `addrs[12]` 本身

2. **分配间接块时**: `addrs[12]` 还是 0 就要先分配一个块, 并且**把它清零**
   (里面的随机数据会被当成有效块号)

3. **分配数据块时**: 新块要清零, 否则文件"自带"一段垃圾 ——
   这是经典的信息泄露 (上一个文件的内容被下一个文件读到)

### 2.2 parse / to_bytes: 磁盘结构与内存结构

```rust
pub const DISK_INODE_SIZE: usize = 64;

impl DiskInode {
    pub fn parse(buf: &[u8]) -> Self;     /* 磁盘字节 -> 内存结构 */
    pub fn to_bytes(&self, buf: &mut [u8]) -> bool;   /* 内存结构 -> 磁盘字节 */
}
```

磁盘格式是**外部约定**, 不能依赖任何编译器行为: 如果不加处理直接把
内存里的 struct 写进磁盘, 编译器的对齐规则会插入填充字节, 不同架构的
字节序也可能不同。所以磁盘格式要**逐字段显式读写**:

```rust
pub fn read_u16_le(buf: &[u8], off: usize) -> u16;
pub fn write_u32_le(buf: &mut [u8], off: usize, v: u32);
```

`to_bytes` 返回 `bool` 而不是 `()`: 万一字段将来放不下, 调用者必须知道
写失败了。磁盘上的 inode 大小是固定的 `DISK_INODE_SIZE`; 内存结构可以
更大 (带缓存、锁、引用计数)。

### 2.3 inode 缓存与生命周期

inode 不是每次用都从磁盘读 —— 那样每个文件操作都要读磁盘。
`inode_get(inum)` 先在缓存里找, 找不到才读磁盘:

```
   inode_get:  ref++      "我正在用这个 inode"
   inode_put:  ref--      "我用完了"
               ref == 0 && nlink == 0  ->  真正删除它
```

`nlink == 0 && ref == 0` 才删除, 这是 Unix "删除正在使用的文件"
语义的核心: unlink 只是把链接数减到 0, 文件要等最后一个使用者
关闭它才真正消失。

## 3. dentry: 目录也是文件

Unix 最优雅的设计之一:

**目录就是一个内容为"目录项数组"的普通文件。**

```rust
pub const DIRENT_SIZE: usize = 4 + 2 + MAX_NAME;   /* inum + type + 定长名字 */

pub fn resolve<'a, F>(path: &[u8], mut lookup: F) -> Result<u32, PathError>
```

于是路径解析变成一个循环: 从根目录开始, 在它的内容里找一个名字,
拿到 inode 号, 再在下一级里找。**你不需要为目录发明任何新的存储机制**。

### 3.1 resolve 的要点

1. **名字必须精确匹配**, 而且 `name` 是**定长、不保证以 `\0` 结尾**的。
   用 `strcmp` 会在名字占满时越界读; 用前缀比较会让 `/test_1`
   匹配到 `/test_10`

2. **`inum == 0` 是空项, 要跳过**, 不能当成"文件结束"

3. `resolve` 接受一个 `lookup` 闭包 —— "怎么读目录"由调用者决定,
   "怎么走路径"留在这里。这样 `dir.rs` 不需要知道磁盘、缓存、
   inode 缓存的存在

4. **当前 inode 不是目录时要立刻报错**, 而不是继续找

返回 `Result<u32, PathError>` 而不是 `Option`: 路径解析失败的原因
有好几种 (不存在、不是目录、太深), 调用者需要区分。

### 3.2 MAX_PATH_DEPTH

路径深度上限是**必须的**: 没有它, 一个构造出来的超长路径会让内核栈上
的递归或临时缓冲溢出。这类"用户可控的深度"和"用户可控的长度"一样,
都是内核漏洞的高发区。

## 4. 测试

考虑到测试的便捷性, 请直接在文件系统初始化 (`mount.rs` 中的挂载逻辑)
的最后位置添加必要的打印与校验逻辑。

### 4.1 QEMU

```bash
cargo xtask run --config riscv64-qemu-virt
```

实现正确时, 根目录会被列出来:

```
[oslab-rs] 块设备自检: 按平台描述初始化
[oslab-rs]   设备: virtio-blk  容量: 2000 扇区
[oslab-rs]   读块 0 成功, 超级块魔数 = 0x10203040  <- 与 mkfs 写入的一致, 块设备通路正常
[oslab-rs] fs   : 超级块 2000 块, 200 个 inode
[oslab-rs] fs   : 根目录内容:
                inum=2  hello
                inum=3  init
```

**验收标准**: 最后那几行 `inum=N name`。这一行同时证明了
inode 读取 (含块映射) 与目录解析都对了。`inum=2 hello` / `inum=3 init`
就是磁盘镜像里那两份文件 —— 顺序由 mkfs 写入的顺序决定, 数字本身
不重要, "名字与 inode 号能对上"才重要。挂载失败时会打印
`[oslab-rs] fs   : 挂载失败 (...)` , 括号里是具体原因 (魔数不对 /
超级块不自洽 / 根目录不是目录), 按它排查即可。

**自检建议**: 放两个名字前缀相同的文件 (`test_1` 与 `test_10`),
确认查找 `test_1` 不会错误地匹配到 `test_10`。这是"比较名字"
最容易犯的错。

---

## 6. 进阶目标（可选）

下面三条**不属于基本验收**：默认流程与本分支 README 的期望输出都不依赖它们。
它们的作用是把这一阶段的内核"做完整一点"——每条都只用到**本章已经给出的东西**，
不碰后面阶段的文件。每条写了"为什么值得做""思路（只说做法，不写代码）""怎么算做到"。

> 动手前先建自己的分支（例如 `git checkout -b my-advanced`）。
> **别破坏默认输出**：进阶改动如果改变了默认运行结果，后面几章的对照实验就失效了；
> 需要改默认行为时，用一个新的 `configs/` 配置或一个运行期开关把它隔开。

---

### 6.1 日志：让文件系统在崩溃后仍然一致

**为什么值得做**：现在一次"创建文件"要改很多处磁盘结构（`crates/kernel/src/fs/bitmap.rs` 的位图、
`crates/kernel/src/fs/inode.rs` 的 inode、`crates/kernel/src/fs/dir.rs` 的目录项）。
中途掉电就会留下不一致：位图说块已被占用、inode 里却没有它。日志用"先写日志、再写原地"把多步更新变成一次原子提交——这是真实文件系统的核心机制之一，
做一遍你就明白为什么它值得牺牲一半写入带宽；Rust 版还顺带让你看清"磁盘格式是跨程序契约"：
格式的两半分居 `xtask/src/mkfs.rs` 与 `crates/kernel/src/fs/inode.rs`，改一处必须同时改另一处。

**思路**：
- 磁盘上加一块日志区（`xtask/src/mkfs.rs` 负责划分，`crates/kernel/src/fs/mount.rs` 的 `Superblock` 记录位置与长度）。
- 用一个"事务"包住一次系统调用里的所有块写：先把将被修改的块写进日志，再写一条**提交记录**；
  提交成功后才把日志内容写回它们真正的位置，最后清掉日志。
  落点是 `crates/kernel/src/fs/bio.rs`——所有块写都从 `bwrite` 走，那是唯一能拦住它们的地方。
- 挂载时检查日志：有已提交的事务就**重放**（把日志内容写回原位），没提交就丢弃；这一步放在
  `crates/kernel/src/fs/mount.rs` 的 `mount` 里，在根目录校验之前。
- 只做**元数据日志**就已经能解决大部分不一致（数据块不进日志），先别贪。
- 三个关键点：提交记录必须**最后写**且带校验（序号或校验和）；日志满时要阻塞新事务（不能覆盖未提交的内容）；
  重放必须幂等（重复挂载安全）。

**怎么算做到**：做一个对比实验——在"改了位图还没改 inode"的时刻强制重启（杀掉 QEMU），没有日志时文件系统挂载后出现不一致，有日志时能自动回到一致状态；正常读写路径的功能与性能损失可接受。

**涉及**：`crates/kernel/src/fs/bio.rs`、`crates/kernel/src/fs/inode.rs`、`crates/kernel/src/fs/bitmap.rs`、`crates/kernel/src/fs/mount.rs`、`xtask/src/mkfs.rs`　**难度**：★★★

### 6.2 VFS：把文件系统抽象成可挂载的接口

**为什么值得做**：现在 `crates/kernel/src/fs/mount.rs` 里的 `Fs` 是一个**具体类型**，
`crates/kernel/src/fs/dir.rs` 的 `resolve` 直接对着它查名字——换一套文件系统就得改路径解析。
Rust 版在这里有一个很自然的目标：把 `Fs` 变成某个 trait 的一个实现，`mount` 变成"把某套实现挂到某个挂载点"。
这是"抽象是否成立的最终检验"：做完之后，加一套新文件系统应该**不用改路径解析**。

**思路**：
- 在 `crates/kernel/src/fs/mod.rs` 里定义一组**窄**接口（挂载/卸载、按名查找、创建、读、写、列目录、删除、取属性）。
  只放真正会被调用的，不要照抄真实内核的宽接口；返回类型用 `Result<_, FsError>`，而不是沿用现在"返回 `Option`、错误信息另开一个通道"的写法。
- 把现有实现包装成第一套实现。`crates/kernel/src/fs/dir.rs` 的 `resolve` 目前接受一个查名字的闭包，
  trait 化之后要决定这里传的是 `&mut dyn FileSystem` 还是继续用闭包——前者更直观，后者能避开借用冲突（`Fs` 持有一个 `dev` 可变借用，这是现成的例子），两种选择各有利弊，先想清楚再动手。
- `Inode` 要么在接口里通用化（最小公共字段 + 私有指针），要么彻底留在实现内部；
  目录项的表示（`DirEntry`）同理。
- 挂载表：路径 + 该点的实现 + 该文件系统的私有数据；现在只有一个全局 `FS` 静态变量，要换成一张表。
  路径解析时按**最长前缀匹配**找到挂载点，跨挂载点的路径（`/mnt/a/b`）要能走通。
- 用一套最简单的"内存文件系统"验证抽象：它没有任何磁盘结构，如果它需要你改 `crates/kernel/src/fs/dir.rs` 的路径解析，说明接口还没抽干净。

**怎么算做到**：现有测试全部照常通过；把内存文件系统挂到某个目录后能创建/读写文件；
跨挂载点路径可解析；挂载列表能在 shell 里打印出来。

**涉及**：`crates/kernel/src/fs/mod.rs`、`crates/kernel/src/fs/mount.rs`、`crates/kernel/src/fs/dir.rs`、`crates/kernel/src/fs/inode.rs`　**难度**：★★★

### 6.3 统一的 page cache

**为什么值得做**：现在每次读文件都走"块大小（512 字节）"的缓冲，大文件读被块粒度限制，
而且"块缓存"与"文件视角"是两套东西。page cache 以**文件页**为单位缓存，
与"文件映射进地址空间"天然配套，也把块缓存放到它该在的位置（它的下层）。
Rust 版还有一件顺手可做的事：`crates/kernel/src/fs/inode.rs` 里的内存结构 `Inode` 已经留了 `refs` / `valid` / `dirty` 三个字段，
但仓库里没有任何地方用它——那正是挂 page cache 的位置。

**思路**：
- 缓存挂在 inode 上：键 =（inode 号，页号），值 = 一个物理页 + 有效位 + 脏位。
  注意"谁拥有那个物理页"：`crates/kernel/src/mm/pmem.rs` 的 `pmem_alloc(Pool::Kernel)` 返回的是裸地址，把它包成一个带 `Drop` 的类型（换出时自动 `pmem_free`）比到处手写释放安全得多——`core::ops::Drop` 在 `no_std` 里同样可用。
- 读写先进缓存：未命中时通过 `crates/kernel/src/fs/bio.rs` 的块接口把**一整页**组装出来（多次块读）。
- 三个要想清楚的点：**部分页写入**（不足一页的写要先读上来再改）；
  **脏页何时写回**（退出时？定时？缓存压力大时？）；**共享**
  （缓存挂在 inode 上，所以同一文件多次打开共享同一份缓存——这正是它与"打开偏移量"的区别，`crates/kernel/src/fs/file.rs` 的 `File::offset` 是每份打开各自一份）。
- 有余力就和前面那章的 mmap / lazy-alloc 接起来：映射一个文件区间时直接把它登记到地址空间，缺页时再从 page cache 拿页。

**怎么算做到**：顺序读一个跨一级间接块的大文件时，块读次数显著下降（打印统计）；
同一页重复读命中缓存；写入后重新挂载内容正确；缓存大小有上限且能回收。

**涉及**：`crates/kernel/src/fs/inode.rs`、`crates/kernel/src/fs/bio.rs`、`crates/kernel/src/fs/file.rs`、`crates/kernel/src/mm/pmem.rs`　**难度**：★★★
