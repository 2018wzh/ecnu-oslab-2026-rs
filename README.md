# LAB-8: 文件系统 之 数据组织与层次结构

**前言**

在lab-7中我们实现了block-level的磁盘管理能力

在此基础上, 本次实验将进一步考虑以下两个问题:

1. 如果数据块的大小大于1个block, 如何组织和管理? (inode)

2. 如何用人类更好理解的层次结构来组织和索引海量数据块? (dentry)

## 代码组织结构

```
ECNU-OSLAB-2026-RS
├── pictures       README使用的图片目录 (CHANGE)
├── README.md      实验指导书 (CHANGE)
├── crates
│   ├── kernel/src/fs
│   │   ├── inode.rs (TODO, 核心工作)
│   │   ├── dentry.rs (TODO, 核心工作)
│   │   ├── mod.rs (TODO, 增加inode初始化逻辑和测试用例)
│   │   └── lab8_examples.rs (NEW, 测试用例)
│   └── uapi/src/disk.rs (CHANGE, 磁盘结构常量)
└── xtask/src/disk.rs (CHANGE, 带根目录的磁盘映像)
```

**标记说明**

**NEW**: 新增源文件, 直接拷贝即可, 无需修改

**CHANGE**: 旧的源文件发生了更新, 直接拷贝即可, 无需修改

**TODO**: 你需要实现新功能 / 你需要完善旧功能

## mkfs: 带根目录的初始磁盘映像

lab-7的初始磁盘映像虽然有inode_bitmap和inode_region, 但是并不存在真正的inode

在本次实验中, 我们首先添加了根目录 (root_inode)

随后, 在根目录之下增加了四个目录项:

- "." 和 "..": 特殊目录项, 用于描述相对路径, 本次实验不展开描述

- "ABCD.txt" 和 "abcd.txt": 循环写入字母表(大写版本和小写版本)的普通文件

请你阅读`crates/uapi/src/disk.rs`和`xtask/src/disk.rs`来理解初始化过程的具体行为, 这将帮助你理解**inode**和**dentry**的概念

## inode: 文件数据组织

从磁盘角度来看, 文件就是由N个block构成的逻辑单元

inode负责记录这样的逻辑结构, 主要依靠**index**字段和**size**字段

**index字段的设计分成三个部分:**

- 0 ~ INODE_INDEX_1 - 1: 直接映射(index[i] = data_block), 控制前面40KB空间 (小型文件)

- INODE_INDEX_1 ~ INODE_INDEX_2 - 1: 一级间接映射(index[i] = index_block), 控制中间8MB空间 (中型文件)

- INODE_INDEX_2 ~ INODE_INDEX_3 - 1: 二级间接映射(index[i] = index_index_block), 控制后面4GB空间 (大型文件)

**这样设计的好处是:**

- 小型文件的访问非常迅速 (直接访问inode就能直接获得data_block序号)

- 能支持很大的文件 (以1个index_index_block和1024个index_block的额外访问开销为代价)

- 中型文件的访问代价可控 (以2个index_block的额外访问开销为代价)

**和之前的内核页表设计进行对比:**

- index是各个文件的私有映射逻辑, 内核页表是系统全局的映射逻辑, 因此内存页面不存在前面热后面冷的性质

- 因此, 页表是确定的三级间接映射, 文件映射则是直接映射、一级间接映射、二级间接映射相结合(按需启用)

**关于size字段(单位是字节)的说明:**

- 对于**INODE_DATA**类型的inode, size除了代表有效的数据量, 还代表[0, size)的逻辑空间已经使用 (没有空洞)

- 对于**INODE_DIR**类型的inode, 我们假设它只使用1个block(index[0]记录), size只代表有效数据量 (接受空洞)

**当你完全理解上述逻辑后, 我们开始进入具体的函数 (in inode.rs)**

首先阅读帮助函数`InodeGuard::free_blocks`, 实现它调用的`free_block_tree`和`InodeGuard::locate_or_add_block`

`InodeGuard::free_blocks`本质是通过`bitmap::free_block`释放inode管理的所有block资源

考虑到树形组织结构, 我们使用递归方法来实现这一点 (调用`free_block_tree`)

注意: 起到索引作用的index_block和index_index_block也要释放

`InodeGuard::locate_or_add_block`负责将逻辑块号转译为物理块号 (例:inode的第一个逻辑块0对应物理块1120)

转译过程中可能遇到目标逻辑块号恰好超过边界 (例:分配了N个逻辑块, 目标逻辑块号为N)

这种时候需要新分配一个物理块, 使得目标逻辑块号合法

比较复杂的情况: 新增data_block可能带来连锁反应 (新增index_index_block和新增index_block)

这一点和页表的生长过程是类似的, 需要你细心和谨慎地处理

之后实现数据流读写逻辑`InodeGuard::read_data`和`InodeGuard::write_data`

它们的共同点在于: 以1个buffer为中间载体, 实现数据在磁盘块和通用内存空间之间的流转

读写参数分别用`ReadDst`和`WriteSrc`表示。内核缓冲区传入切片, 用户缓冲区则传入`UserAddr`和长度, 通过前章的用户内存复制接口访问。

需要注意的一点: 写入逻辑可能涉及inode的修改 (包括index和size), 文件大小不能超过INODE_MAX_SIZE (size为32位无符号数, 最多表示4294967295字节)

## inode: 生命周期管理

我们使用**CACHE**来管理内存中的inode资源, 通过**CACHE_LOCK**来保护它

这种组织结构在`mmap`、`proc`、`buffer`等地方多次遇到, 这里也是类似的 (只是没有选择使用双向循环链表做加速)

请你完成`inode::init`来初始化资源, 并放入`fs::init`的合适位置

下一个需要思考的问题是: **inode in disk** 与 **inode in memory**的区别与互相更新

```rust
/* 磁盘上的索引节点字段(按64字节格式编解码) */
pub struct DiskInode {
    pub kind: u16,                   // 文件类型
    pub major: u16,                  // 主设备号
    pub minor: u16,                  // 次设备号
    pub nlink: u16,                  // 链接数
    pub size: u32,                   // 文件数据长度(字节)
    pub index: [u32; 13],            // 数据存储位置(10+2+1)
}

/* 内存里的索引节点 */
pub struct Inode {
    info: DiskInode,                 // 持久化信息 (lock保护)
    number: u32,                    // inode序号 (活跃引用期间不变)
    refs: usize,                    // 引用数 (CACHE_LOCK保护)
    valid: bool,                    // info的有效性 (lock保护)
    lock: SleepLock,                // 睡眠锁
}
```

相比**DiskInode**, **Inode** 增加了四个字段:

- valid: CACHE miss后返回的空闲inode, 它的info是无效的, 需要特殊标记

- refs: CACHE中的inode可以被多个使用者关注, 需要记录一下引用数

- number: 进行**DiskInode**和**Inode**的互相更新时, 需要知道磁盘中的inode位置

- lock: 保证资源的按需共享, 由于磁盘读写非常耗时, 所以采用睡眠锁而非自旋锁

之后请你实现`InodeGuard::rw`完成磁盘inode_region中的inode和内存CACHE中的inode的互相更新。磁盘字段按固定偏移用小端格式读写, 不直接复制内存结构体。

- 初始化时: 通常是 磁盘->内存 的更新流 (读入一个新的inode)

- 修改时: 通常是 内存->磁盘 的更新流 (inode中的字段做了修改, 需要写回)

下面实现典型的inode生命周期控制函数 (按照从生到死的过程)：

- `inode::create`: 认为某个inode原本不存在, 先在内存里创建副本, 之后写入inode_region

- `inode::get`: 认为某个inode存在于内存CACHE或者磁盘inode_region, 获取使用权

- `InodeRef::dup`: 复制inode使用权 (例如执行fork操作时)

- `InodeRef::lock`: 获取睡眠锁并返回InodeGuard, 以保证独占inode

- `InodeGuard::drop`: 释放守卫时解锁, 以支持共享inode

- `InodeRef::drop`: 释放引用时减少refs, 可能触发inode的磁盘删除操作

- `InodeGuard::delete`: 删除磁盘里的某个inode, 并释放它管理的data_block资源

最后, 我们提供了`InodeGuard::print`函数来输出某个inode的具体信息

## dentry: 从数字索引到字符串索引

**有了inode的文件系统世界是什么样的呢?**

- 我们可以先用**inode_num**搜索**inode_region**, 找到某个inode (定位元数据)

- 再通过inode里的索引信息, 按照逻辑顺序找到它管理的若干**block** (定位数据)

对于计算机来说, 这套逻辑已经足够高效了, 它建立了从数字到离散数据流的映射关系

对人来说, 还存在一个致命的问题:**inode_num**太抽象了, 人类很难记住它的含义, 人更好理解的是**name**

我们可以把 **name=hello.c** 理解为一个"hello world"程序, 但是不能把 **inode_num=15** 理解成任何东西

**因此, 我们决定增加一层映射逻辑: name -> inode_num**

```text
目录项(64 Byte):
    name: [u8; NAME_BYTES]          // 文件名, 占前60字节
    inode_num: u32                 // 索引节点序号, 占后4字节, 小端编码
```

更具体的设计方案:

- 我们将第一个inode设为**根节点**, 记住它的inode_num为**ROOT_INODE(0)**

- 根节点包括 `BLOCK_SIZE/64` 个**dentry槽位**

- 其他inode都通过注册1个dentry链接到根节点上, 形成**1-N**的二层树形结构

- 查找逻辑从: inode_num->数据 变成了 ROOT_INODE->name->inode_num->数据

用户只需要记住根节点的inode序号和目标inode的名称即可, 不需要知道目标inode的序号

请你实现dentry的槽位管理逻辑:

- `dentry::search`: 在目录中查找某个dentry, 找不到时返回Err(()), 不能用合法的根目录编号0表示失败

- `dentry::create`: 创建1个新的dentry

- `dentry::delete`: 删除1个旧的dentry

我们还提供了一个`dentry::print`用于打印某个目录下的所有有效dentry (目录项的第一个名称字节不为0说明有效)

## path: 从扁平化到层次化

**1-N**的扁平化树形结构在inode数量较小时是好用的

随着inode数量的增长: 一方面, 根节点的槽位不够用; 另一方面, 不方便人做分类查找和管理

因此, 我们要将扁平化的树拓展为层次化的树, 支持`1-N-M-K...`的多层结构

管理这样的结构需要多个目录类型的inode (仅靠根节点是不够的)

描述树上的一个节点需要使用"文件路径(path)", 本次实验我们只考虑绝对路径

例如: /AAA/BBB/CCC/file.txt、/AABB/CC、////AAA///CC//BB/file.txt等

以/AAA/BBB/CCC.txt为例, 解释**路径解析**过程:

- step-0: 通过ROOT_INODE(0)获得root_inode

- step-1: 查询root_inode的dentry槽位, 发现"AAA"对应的inode_num为A1

- step-2: 通过A1获得AAA_inode

- step-3: 查询AAA_inode的dentry槽位, 发现"BBB"对应的inode_num为B1

- step-4: 通过B1获得BBB_inode

- step-5: 查询BBB_inode的dentry槽位, 发现"CCC.txt"对应的inode_num为C1

- step-6: 通过C1获得CCC_inode, 进行后续的读写行为

我们提供了`dentry::element`来逐步处理**path**和提取**name**, 请你先理解它的工作逻辑。Rust中的路径用不带结尾NUL的字节切片表示

你需要利用`dentry::element`、`inode::get`、`dentry::search`等函数来实现`dentry::resolve`

这个函数完成了从**path**到**目标inode**的翻译过程, 是`dentry::lookup`和`dentry::parent`的底层

## 测试用例

考虑到测试的便捷性, 请直接在`fs::init`的最后位置添加一组测试逻辑, 替换原有的`lab8_examples::lab8_examples()`调用。

**测试1: inode的访问 + 创建 + 删除**

测试现象示意:

![pic](./pictures/01.png)

```rust
    /* fs::init in fs/mod.rs */
    use crate::fs::{bitmap, inode};
    use oslab_uapi::disk::*;
    // SAFETY: fs::init已完成超级块初始化, 此后只读。
    let sb = unsafe {
        (&*(&raw const crate::fs::SUPERBLOCK))
            .as_ref()
            .expect("fs init")
    };

    crate::println!("============= test begin =============");
    let rooti = inode::get(ROOT_INODE);
    rooti.lock().print("root");
    /* 第一次查看bitmap */
    bitmap::print(sb, false);
    let ip_1 = inode::create(INODE_DIR, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let ip_2 = inode::create(INODE_DATA, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let mut g1 = ip_1.lock();
    let mut g2 = ip_2.lock();
    let duplicate = ip_2.dup();
    g1.print("dir");
    g2.print("data");
    /* 第二次查看bitmap */
    bitmap::print(sb, false);
    g1.info_mut().nlink = 0;
    g2.info_mut().nlink = 0;
    drop(g1);
    drop(g2);
    drop(ip_1);
    drop(ip_2);
    /* 第三次查看bitmap */
    bitmap::print(sb, false);
    drop(duplicate);
    /* 第四次查看bitmap */
    bitmap::print(sb, false);
    crate::println!("============= test end =============");
    loop {
        core::hint::spin_loop();
    }
```

**测试2: 写入和读取inode管理的数据**

测试现象示意:

![pic](./pictures/02.png)

```rust
    /* fs::init in fs/mod.rs */
    use crate::fs::inode::{self, ReadDst, WriteSrc};
    use crate::mem::{PAGE_SIZE, pmem};
    use oslab_uapi::disk::*;

    crate::println!("============= test begin =============");
    /* 小批量读写测试 */
    let mut small_src = [0u8; 40];
    let mut small_dst = [0u8; 40];
    for i in 0..10u32 {
        small_src[i as usize * 4..i as usize * 4 + 4].copy_from_slice(&i.to_le_bytes());
    }
    let ip_1 = inode::create(INODE_DATA, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let mut g1 = ip_1.lock();
    g1.print("small_data");
    crate::println!("writing data...");
    for offset in (0..400 * 40).step_by(40) {
        assert_eq!(
            g1.write_data(offset, WriteSrc::Kernel(&small_src)),
            40,
            "write fail 1"
        );
    }
    g1.print("small_data");
    assert_eq!(
        g1.read_data(120 * 40 + 4, ReadDst::Kernel(&mut small_dst)),
        40,
        "read fail 1"
    );
    crate::print!("read data:");
    for b in small_dst.chunks_exact(4) {
        crate::print!(" {}", u32::from_le_bytes(b.try_into().unwrap()));
    }
    crate::println!();
    g1.info_mut().nlink = 0;
    drop(g1);
    drop(ip_1);
    /* 大批量读写测试 */
    // 分别申请五页，逐页验证连续；不能对用户地址作此处理。
    let big = pmem::alloc(true);
    for i in 1..5 {
        assert_eq!(pmem::alloc(true), big + i * PAGE_SIZE, "contiguous fail");
    }
    {
        // SAFETY: 五个内核页均独占且刚刚验证连续，切片在 free 前结束使用。
        let big_src = unsafe { core::slice::from_raw_parts_mut(big as *mut u8, 5 * PAGE_SIZE) };
        for (i, b) in big_src.iter_mut().enumerate() {
            *b = b'A' + (i % 8) as u8;
        }
        let ip_2 = inode::create(INODE_DATA, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
        let mut g2 = ip_2.lock();
        g2.print("big_data");
        crate::println!("writing data...");
        let cut_len = (PAGE_SIZE * 4 + 1110) as u32; // 17494
        for offset in (0..cut_len * 10000).step_by(cut_len as usize) {
            assert_eq!(
                g2.write_data(offset, WriteSrc::Kernel(&big_src[..cut_len as usize])),
                cut_len as usize,
                "write fail 2"
            );
        }
        g2.print("big_data");
        let mut big_dst = [0u8; 9];
        // 尾部读取必须是仍持锁的大文件 ip_2。
        assert_eq!(
            g2.read_data(cut_len * 10000 - 8, ReadDst::Kernel(&mut big_dst[..8])),
            8,
            "read fail 2"
        );
        crate::println!(
            "read data: {}",
            core::str::from_utf8(&big_dst[..8]).unwrap()
        );
        g2.info_mut().nlink = 0;
        drop(g2);
        drop(ip_2);
    }
    for i in 0..5 {
        // SAFETY: 对应本测试独占申请的五页，数据引用均已结束。
        unsafe {
            pmem::free(big + i * PAGE_SIZE, true);
        }
    }
    crate::println!("============= test end =============");
    loop {
        core::hint::spin_loop();
    }
```

**测试3: 目录项的增加、删除、查找操作**

测试现象示意:

![pic](./pictures/03.png)

```rust
    /* fs::init in fs/mod.rs */
    use crate::fs::{
        dentry,
        inode::{self, ReadDst},
    };
    use oslab_uapi::disk::*;

    crate::println!("============= test begin =============");
    let rooti = inode::get(ROOT_INODE);
    /* 搜索预置的dentry */
    let mut gr = rooti.lock();
    let inode_num_1 = dentry::search(&mut gr, b"ABCD.txt").expect("invalid inode num");
    let inode_num_2 = dentry::search(&mut gr, b"abcd.txt").expect("invalid inode num");
    let inode_num_3 = dentry::search(&mut gr, b".").expect("invalid inode num");
    dentry::print(&gr).unwrap();
    drop(gr);
    let ip_1 = inode::get(inode_num_1);
    let mut g1 = ip_1.lock();
    let ip_2 = inode::get(inode_num_2);
    let mut g2 = ip_2.lock();
    let ip_3 = inode::get(inode_num_3);
    let g3 = ip_3.lock();
    g1.print("ABCD.txt");
    g2.print("abcd.txt");
    g3.print("root");
    let mut tmp = [0u8; 10];
    assert_eq!(
        g1.read_data(0, ReadDst::Kernel(&mut tmp[..9])),
        9,
        "read fail 1"
    );
    crate::println!("read data: {}", core::str::from_utf8(&tmp[..9]).unwrap());
    assert_eq!(
        g2.read_data(0, ReadDst::Kernel(&mut tmp[..9])),
        9,
        "read fail 2"
    );
    crate::println!("read data: {}", core::str::from_utf8(&tmp[..9]).unwrap());
    drop(g1);
    drop(g2);
    drop(g3);
    drop(ip_1);
    drop(ip_2);
    drop(ip_3);
    /* 创建和删除dentry */
    let mut gr = rooti.lock();
    let new_dir = inode::create(INODE_DIR, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let offset = dentry::create(&mut gr, new_dir.number(), b"new_dir").unwrap();
    let number = dentry::search(&mut gr, b"new_dir").unwrap();
    crate::println!(
        "new dentry offset = {}\nnew dentry inode_num = {}",
        offset,
        number
    );
    dentry::print(&gr).unwrap();
    assert_eq!(
        number,
        dentry::delete(&mut gr, b"new_dir").unwrap(),
        "inode num is not equal"
    );
    dentry::print(&gr).unwrap();
    drop(gr);
    drop(rooti);
    crate::println!("============= test end =============");
    loop {
        core::hint::spin_loop();
    }
```

**测试4: 文件路径的解析**

测试现象示意:

![pic](./pictures/04.png)

```rust
    /* fs::init in fs/mod.rs */
    use crate::fs::{
        dentry,
        inode::{self, ReadDst, WriteSrc},
    };
    use oslab_uapi::disk::*;

    crate::println!("============= test begin =============");
    /* 准备测试环境 */
    let rooti = inode::get(ROOT_INODE);
    let ip_1 = inode::create(INODE_DIR, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let ip_2 = inode::create(INODE_DIR, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let ip_3 = inode::create(INODE_DATA, INODE_MAJOR_DEFAULT, INODE_MINOR_DEFAULT);
    let mut gr = rooti.lock();
    let mut g1 = ip_1.lock();
    let mut g2 = ip_2.lock();
    let mut g3 = ip_3.lock();
    dentry::create(&mut gr, ip_1.number(), b"AABBC").expect("dentry_create fail 1");
    dentry::create(&mut g1, ip_2.number(), b"aaabb").expect("dentry_create fail 2");
    dentry::create(&mut g2, ip_3.number(), b"file.txt").expect("dentry_create fail 3");
    let tmp1 = b"This is file context!\0";
    g3.write_data(0, WriteSrc::Kernel(tmp1));
    gr.rw(true);
    g1.rw(true);
    g2.rw(true);
    drop(gr);
    drop(g1);
    drop(g2);
    drop(g3);
    drop(rooti);
    drop(ip_1);
    drop(ip_2);
    drop(ip_3);
    let path = b"///AABBC///aaabb/file.txt";
    let ip_4 = dentry::lookup(path).ok().expect("invalid ip_4");
    let (ip_5, name) = dentry::parent(path).ok().expect("invalid ip_5");
    let end = name.iter().position(|b| *b == 0).unwrap_or(NAME_BYTES);
    crate::println!(
        "get a name = {}",
        core::str::from_utf8(&name[..end]).unwrap()
    );
    let mut g4 = ip_4.lock();
    let g5 = ip_5.lock();
    g4.print("file.txt");
    g5.print("aaabb");
    let mut tmp2 = [0u8; 32];
    g4.read_data(0, ReadDst::Kernel(&mut tmp2));
    let end = tmp2.iter().position(|b| *b == 0).unwrap_or(32);
    crate::println!("read data: {}", core::str::from_utf8(&tmp2[..end]).unwrap());
    drop(g4);
    drop(g5);
    drop(ip_4);
    drop(ip_5);
    crate::println!("============= test end =============");
```

**尾声**

本次实验我们实现了两个文件系统的基石: inode 和 dentry

它们分别定义了文件系统的数据组织逻辑和层次化逻辑, 希望能帮助你理解文件系统的构建逻辑

在lab-9中, 我们将先基于这两块基石构建**普通文件和目录文件**的管理逻辑

之后, 秉持**一切皆文件**的Linux设计哲学, 我们还将介绍一类特殊的文件——**设备文件**

最后, 我们还将讨论进程模块与文件系统的关系, 并补全进程模块的最后一块拼图——**proc::exec::exec**

**lab-9 既是文件系统的最终章、也是内核全系统的粘合剂、还是迄今为止最困难的终极考核!**

## 进阶目标

### 日志文件系统

创建一个文件时, 位图、inode和目录都可能需要更新。如果写到一半突然断电, 这些信息就可能互相矛盾。日志文件系统会先记录一组相关修改, 让重启后的内核能够判断这组操作是否完成, 并据此恢复。

请你先以创建文件为例, 确定哪些修改需要一起完成, 再尝试设计日志记录、提交标志和恢复顺序。在不同写入位置模拟断电, 重启后检查目录、链接数和位图是否一致, 验证后再扩大日志覆盖的操作范围。

### VFS 挂载

不同文件系统组织磁盘数据的方式可能不同, 但都需要提供文件查找、目录遍历等操作。VFS用一组共同接口连接这些实现, 挂载则让一个文件系统出现在另一个文件系统的目录下。

请你尝试从inode、目录和路径操作中提取共同接口, 先在一个目录上挂载第二个只读实例。观察跨挂载点的路径解析, 测试根目录、`.`、`..`以及仍有引用时的卸载, 注意区分不同文件系统中相同的inode编号。完成lab-9的文件接口后, 可以继续验证文件访问。

### 统一 page cache

本次实验按磁盘块缓存数据, 文件读写需要先把文件内偏移换成块号。如果改为按文件和页号缓存, 文件读写与文件内存映射就可以使用同一份内存副本。修改过但尚未写回磁盘的页面称为脏页, 回收前需要处理这些修改。

请你尝试设计这样的缓存, 可以先确定如何标识文件页、记录脏页和安排写回, 再考虑它与现有块缓存的关系。结合后续文件与映射实验, 观察不同方式访问同一文件时能否读到一致的数据, 并检查写回失败或页面仍在使用时该如何处理回收。
