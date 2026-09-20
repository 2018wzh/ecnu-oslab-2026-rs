# LAB-7: 文件系统 之 磁盘管理

**前言**

本次实验我们将围绕磁盘管理构建文件系统的基础设施

1. 首先讨论QEMU启动时的输入参数disk.img是如何构建的

2. 随后讨论以block为基本单位的磁盘读写如何实现, 包括驱动本身+OS提供的配合

3. 随后讨论磁盘与内存进行数据交换的桥梁--缓冲系统(buffer)

4. 最后讨论磁盘上bitmap区域的管理方法

## 代码组织结构

```
ECNU-OSLAB-2026-RS
├── pictures       README使用的图片目录 (CHANGE, 日常更新)
├── README.md      实验指导书 (CHANGE, 日常更新)
├── crates
│   ├── uapi/src
│   │   ├── disk.rs (NEW, 磁盘布局)
│   │   └── lib.rs (CHANGE, 系统调用号)
│   ├── drivers/src
│   │   ├── block.rs (NEW, QEMU磁盘驱动)
│   │   └── sd.rs (NEW, VisionFive2磁盘驱动)
│   └── kernel/src
│       ├── mem/kvm.rs (TODO, 磁盘映射与内核地址翻译)
│       ├── trap/mod.rs (TODO, 磁盘中断使能与处理)
│       ├── proc/schedule.rs (TODO, 首进程中初始化文件系统)
│       ├── syscall
│       │   ├── mod.rs (CHANGE, 系统调用分派)
│       │   └── disk.rs (TODO, 磁盘测试系统调用)
│       ├── fs
│       │   ├── block.rs (TODO, 磁盘寄存器映射)
│       │   ├── buffer.rs (TODO, 缓冲区管理)
│       │   ├── bitmap.rs (TODO, bitmap相关操作)
│       │   ├── tokens.rs (NEW, 跨系统调用保存buffer守卫)
│       │   └── mod.rs (TODO, 文件系统初始化)
│       └── main.rs (TODO, 增加block::init)
├── xtask/src/disk.rs (NEW, 磁盘映像初始化)
└── user/src
    ├── bin/init.rs (按测试需求修改)
    └── syscall.rs (CHANGE, 用户系统调用接口)
```

**标记说明**

**NEW**: 新增源文件, 直接拷贝即可, 无需修改

**CHANGE**: 旧的源文件发生了更新, 直接拷贝即可, 无需修改

**TODO**: 你需要实现新功能 / 你需要完善旧功能

## 磁盘的初始状态--disk.img如何构建

在QEMU中引入磁盘这种新的外设, 需要增加启动参数。请关注**xtask/src/run.rs**中的以下部分:

```text
-global virtio-mmio.force-legacy=false
-drive file=target/riscv64-qemu-virt/disk.img,if=none,format=raw,id=x0
-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
```

它定义了磁盘在启动时的初始状态为disk.img, 同时启动了一个虚拟磁盘设备作为disk.img的载体

**disk.img不是凭空产生的,它是如何构建的呢？**

请你关注**xtask/src/disk.rs**和**crates/uapi/src/disk.rs**源文件

简单来说, 它负责创建和打开一个文件, 并向这个文件写入一些信息进行文件系统格式化

通过`OpenOptions::open`、`write_all`和`set_len`这组常见的文件接口来实现 (注意, 它不是基于我们实现的内核, 而是Linux)

具体来说, 磁盘可以被看作以block为基本单位的长数组, **crates/uapi/src/disk.rs**规定了磁盘布局结构如下:

**[ superblock | inode bitmap | inode region | data bitmap | data region ]**

- block是磁盘的基本逻辑单位, 磁盘由若干block构成, block的大小规定为**BLOCK_SIZE**, 这里与**PAGE_SIZE**保持一致

- 第1部分由**1个**block构成, 被称为超级块, 记录了文件系统和磁盘的相关信息(布局、魔数、块大小等), 是最重要的元数据

- 第2、3部分描述文件系统元数据, 第4、5部分描述文件系统数据, 他们都是**element_bitmap + element_region**的结构

- 第3部分包括N个inode, 第2部分描述第3部分各个inode元素是否分配出去了 (bit为1代表已分配, bit为0代表未分配)

- 第5部分包括M个data block, 第4部分描述第5部分各个data block元素是否分配出去了 (bit为1代表已分配, bit为0代表未分配)

通过修改**N_INODE**和**N_DATA_BLOCK**, 我们可以控制元数据资源池和数据资源池的大小, 进而影响disk.img的大小

初始化结束后, disk.img中的**superblock**完成了设置, **inode bitmap**和**data bitmap**全部清零

使用`cargo xtask disk --config riscv64-qemu-virt`创建镜像, 随后可用`cargo xtask run --config riscv64-qemu-virt`启动。镜像默认位于`target/riscv64-qemu-virt/disk.img`, 运行命令不会替你格式化已有镜像。需要清空重建时, 在disk命令后加上`--force`。

VisionFive2使用microSD, 镜像可用`cargo xtask disk --config riscv64-visionfive2`生成。将disk.img写入从扇区2097152开始的实验区, 该区域至少需要10494296个512字节扇区, 不应与启动分区重叠。内核用`cargo xtask image --config riscv64-visionfive2`生成kernel.itb, 沿用lab-1的U-Boot加载步骤启动。具体布局与写入方法见[开发板磁盘说明](docs/visionfive2-sd.md)。

注意: 在本次实验中, 你只需要知道**inode region**是一个区别于**data region**的区域即可, 不需要对inode有细致了解

## 构建block-level的读写能力

构建disk.img后, 我们还需要构建读写它的基本能力, 才能实现数据的持久化存储

前面提到过, 磁盘的基本管理单位是block, 因此我们首先考虑如何构建block-level的读写能力

我们之前学习过另一种具备读写能力的外设--UART(串口), 可以获得以下启示:

- 需要**磁盘驱动程序**, 通过一系列寄存器操作实现读写能力

- 需要与OS的陷阱子系统密切配合, 实现中断响应函数 (磁盘操作很费时, 本实验采用中断方式)

**1. 首先讨论磁盘驱动程序的部分 (了解即可)**

驱动程序非常复杂, 且和设备寄存器耦合严密, 不是学习的重点, 只需要知道它提供的接口即可

请你查看**crates/kernel/src/fs/block.rs**源文件, 它为QEMU的VirtIO驱动和VisionFive2的SD驱动提供统一接口, 包括以下几个函数:

```text
// block.rs: 以block为单位的磁盘读写能力
pub fn init() -> Result<(), ()>; // 磁盘初始化
pub fn rw(block: u32, data: &mut [u8; BLOCK_SIZE], write: bool) -> Result<(), ()>; // 磁盘读写
pub fn interrupt(); // 磁盘中断处理
```

- `block::init`与磁盘进行通信并让它进入READY状态

- `block::rw`提供了以block为单位的读写能力, 供buffer子系统使用

- `block::interrupt`是磁盘中断处理流程, 当磁盘完成一次I/O时会通过中断系统提醒OS, 唤醒等待磁盘资源的进程

**2. 再讨论OS如何与磁盘驱动配合 (需要你做)**

- 系统初始化(**crates/kernel/src/main.rs**): 主核调用`block::init`, 完成后再使能磁盘中断

- 内存系统(**crates/kernel/src/mem/kvm.rs**): 需要在内核页表初始化时调用`block::map`, 完成磁盘相关寄存器的映射工作。VisionFive2还需要映射用于DMA缓存维护的CCACHE寄存器

- 内存系统(**crates/kernel/src/mem/kvm.rs**): `block::rw`通过`kvm::translate`翻译内核栈中请求头的地址, 请实现这个函数。`kvm::getpte`仍需传入有效页表, 不用空指针表示内核页表

- 陷阱系统(**crates/kernel/src/trap/mod.rs**): 为`BLOCK_IRQ`设置PLIC优先级, 并在各核使能磁盘中断

- 陷阱系统(**crates/kernel/src/trap/mod.rs**): 在外设中断处理流程中增加磁盘中断的处理分支, 调用`block::interrupt`后完成中断响应

## 建立磁盘与内存的数据交换桥梁--缓冲系统 (buffer)

```rust
/* 以Block为单位在内存和磁盘间传递数据 */
pub struct Buffer {
    pub block: u32,                  // 磁盘内block序号, 由CACHE_LOCK保护
    pub refs: usize,                 // 引用数, 由CACHE_LOCK保护
    pub valid: bool,                 // 数据是否有效, 由睡眠锁保护
    pub disk: bool,                  // 保留字段
    pub io_result: i32,              // 保留字段
    pub data: *mut u8,               // block数据, 内容由睡眠锁保护
    pub lock: SleepLock,             // 睡眠锁
    pub prev: *mut Buffer,           // 链表指针, 由CACHE_LOCK保护
    pub next: *mut Buffer,
}
```

首先, 数据要从内存写入磁盘, 需要将内存缓冲区与磁盘中block的序号进行绑定, 指导`block::rw`的工作

因此, **Buffer**需要包括**block: u32**和**data: *mut u8**来记录这种绑定关系

此外, 磁盘是共享资源, 可能有多个进程同时访问一个block的情况

因此, 需要引入睡眠锁**lock**保证高效的有序访问, 引入计数器**refs**防止过早释放资源

最后, **valid**用于判断缓存内容是否有效。Rust的请求状态和结果由**block.rs**中的请求数组记录, Buffer中保留的**disk**和**io_result**不承担这项工作

```rust
pub static mut CACHE: [Buffer; N_BUFFER] = [const { Buffer::EMPTY }; N_BUFFER];
pub static mut ACTIVE: Buffer = Buffer::EMPTY;
pub static mut INACTIVE: Buffer = Buffer::EMPTY;
pub static CACHE_LOCK: SpinLock = SpinLock::UNINIT;
```

类似之前**mmap**的管理方式, **Buffer结构体资源**被组织为两个带头节点的双向循环链表

**1. 资源初始化 (buffer::init)**

非活跃链表(以**INACTIVE**为头节点)中所有元素的refs都等于0 (无人引用)

活跃链表(以**ACTIVE**为头节点)中所有元素的refs都大于0 (有人引用)

因此, 在初始化时, CACHE中所有buffer的refs设为0, block设为**UNUSED**

随后, 将所有初始化的buffer插入非活跃链表 (我们希望第一个buffer最后位于INACTIVE.next)

**2. 资源获取 (buffer::get)**

buffer在链表间/链表内的移动遵守LRU原则: 最活跃的资源位于(*head).next, 最不活跃的资源位于(*head).prev

![pic](./pictures/01.png)

当上层尝试获取某个block对应的buffer时(如图片所示):

- 我们首先尝试在活跃链表中寻找 (从(*head).next开始), 找到后将它移动到活跃链表的(*head).next

- 如果找不到则尝试在不活跃链表中开始寻找 (从(*head).next开始), 找到后将它移动到活跃链表的(*head).next

- 如果还是找不到, 说明缓存失败: 将非活跃链表尾部的buffer拿出来, 设置block, 移动到活跃链表的(*head).prev

取得睡眠锁后, 如果valid为true, 就可以返回BufferGuard。否则需要先去磁盘中读入目标block。即使命中同一块号, 数据页被回收后也需要重新读盘。

注意: 通过buffer::get获取的buffer, 对应的refs应该+1, 记录被使用的次数。应在缓存锁内增加引用数, 然后释放缓存锁再等待睡眠锁, 避免等待时buffer被回收

**3. 资源释放 (BufferGuard::drop)**

BufferGuard释放时先释放睡眠锁, 再在缓存锁内将refs减1, 如果减到0, 则移动到不活跃链表的(*head).next

![pic](./pictures/02.png)

**4. 关于buffer控制的物理内存的申请和释放**

我们按照自动申请, 手动释放的原则管理buffer控制的物理内存资源 (大小为BLOCK_SIZE, 与物理页一样大)

具体来说:

- 在`buffer::get`获取不活跃链表中的元素时, 检查buf.data是否为空指针, 是的话申请一个物理页

- 在`buffer::freemem`中扫描不活跃链表中的若干最不活跃元素, 尝试释放buffer_count个物理页, 并清除valid。发布或清空data指针时也需要持有缓存锁

**5. 基于buffer的block读写**

`BufferGuard::read` 和 `BufferGuard::write` 的底层都是 `block::rw`

BufferGuard持有睡眠锁, 可以通过`data()`和`data_mut()`访问数据, 再调用`read()`或`write()`进行磁盘操作

**6. 典型的buffer使用方法**

```rust
/* 常规流程 */
let mut buf = buffer::get(block_num).expect("buffer get");
do_something_in_buf_data();
buf.write(); // 也可以只读不修改
drop(buf);

/* 一段时间后可能存在大量无用缓存 */
buffer::freemem(buffer::N_BUFFER);
```

## 使用buffer: 读入superblock

让我们来利用刚刚建立的缓冲系统做点重要的事情: 读入超级块

**首先考虑读入的时机: 可以在kernel_main函数中完成吗?**

不能, 因为磁盘读入会触发`schedule::sleep`和`schedule::wakeup`

所以需要在用户进程的上下文中执行, 而不是在初始化过程中执行

**什么时刻是最早的时机呢?**

初始化过程中通过`proc::make_first`准备好了**PROCZERO**, 并将它的context.ra设为`schedule::first_return`

之后初始化过程进入调度器逻辑(`schedule::scheduler`), 将控制流切换到**PROCZERO**

因此, 最早的时机就是**PROCZERO**第一次进入`schedule::first_return`时!

我们在这里释放调度器交来的进程锁, 再由PROCZERO调用一次`fs::init`进行文件系统初始化, 目前主要用于初始化缓冲系统、调用`tokens::init`初始化令牌存储和读入superblock。读入块0后, 按小端格式解码并检查各区域的位置和大小。

考虑到debug的方便性, 请在读入superblock后输出磁盘布局信息 (通过`Superblock::print`)

## 使用buffer: bitmap管理

bitmap的管理以bit为基本粒度, 因此需要单独开辟一套管理逻辑

- 当申请一个data block或inode时, 对应bitmap的某个bit被置为1

- 当释放一个data block或inode时, 对应bitmap的对应bit被置为0

请你基于buffer来实现以下函数:

```text
pub fn alloc_block() -> u32;
pub fn alloc_inode() -> u32;
pub fn free_block(block_num: u32);
pub fn free_inode(inode_num: u32);
```

**它们的共同逻辑:**

- `bitmap::search_and_set`: 在1个bitmap_block中从头向后扫描bit流, 找到第一个为0的bit, 设置为1并返回Some(索引号), 块内无空位时返回None

- `bitmap::clear`: 将bitmap_block中的某个bit设为0

**需要注意的问题:**

- bitmap区域可能横跨多个block, 寻找空闲bit时需要遍历

- bitmap区域的最后一个block可能只用了一部分, 寻找空闲bit时需要传入有效范围

- 细心一点, 可以通过逐字节遍历和逐bit位运算来寻找空闲bit

`bitmap::alloc_block`返回文件系统内的绝对块号, `bitmap::alloc_inode`返回从0开始的inode编号。

## 增加系统调用

我们需要增加以下11个系统调用的支持, 以支持后面的用户态测试用例

```rust
pub const SYS_ALLOC_BLOCK: usize = 11;  // 从data_bitmap申请1个block (测试bitmap::alloc_block)
pub const SYS_FREE_BLOCK: usize = 12;   // 向data_bitmap释放1个block (测试bitmap::free_block)
pub const SYS_ALLOC_INODE: usize = 13;  // 从inode_bitmap申请1个inode (测试bitmap::alloc_inode)
pub const SYS_FREE_INODE: usize = 14;   // 向inode_bitmap释放1个inode (测试bitmap::free_inode)
pub const SYS_SHOW_BITMAP: usize = 15;  // 输出目标bitmap的状态
pub const SYS_GET_BLOCK: usize = 16;    // 获取1个描述block的buffer (测试buffer::get)
pub const SYS_READ_BLOCK: usize = 17;   // 将缓冲块数据拷贝到用户空间
pub const SYS_WRITE_BLOCK: usize = 18;  // 基于用户地址空间更新缓冲块数据并写入磁盘 (测试BufferGuard::write)
pub const SYS_PUT_BLOCK: usize = 19;    // 释放1个描述block的buffer (测试BufferGuard::drop)
pub const SYS_SHOW_BUFFER: usize = 20;  // 输出buffer链表的状态
pub const SYS_FLUSH_BUFFER: usize = 21; // 释放非活跃链表中buffer持有的物理内存资源 (测试buffer::freemem)
```

请你结合**crates/kernel/src/syscall/disk.rs**的注释和后面给出的测试用例来理解这些系统调用的输入输出

几乎都是先做参数读取, 然后调用对应的实现函数, 请你实现这些系统调用, 这里不做详细介绍

`get_block`返回的数值用来标识内核中的buffer, 用户程序只需保存并原样传回, 不要将它当作用户地址访问。每次获取后只归还一次, 尚未归还时不要调用fork或exit。内核用`tokens::retain`保存BufferGuard, 用`tokens::with`借用它完成读写, 最后用`tokens::release`归还。`read_block`和`write_block`每次复制完整的BLOCK_SIZE字节。

`show_bitmap`的参数0表示data, 1表示inode, 而`bitmap::print`的布尔参数true表示data, 调用时需要作相应转换。`flush_buffer`成功时返回0, 不直接返回`buffer::freemem`回收的页数。

## 测试用例

测试开始前, 请将**crates/kernel/src/fs/buffer.rs**中的**N_BUFFER**从16384改成**N_BUFFER_TEST**, 方便测试。下面三组程序分别替换**user/src/bin/init.rs**。测试3会直接写入块5000, 请使用新建的专用测试镜像。

测试用例包括三个部分:

1. 什么都不做, 测试superblock信息能否正常输出, 检验磁盘和缓冲系统的基本能力

2. 测试bitmap中资源申请和释放的正确性

3. 测试缓冲系统的LRU管理逻辑是否生效

**test-1**

```rust
#![no_std]
#![no_main]
use oslab_user::syscall as sys;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    // SAFETY: 字符串以NUL结尾, 在调用期间有效。
    unsafe { sys::print_str(c"hello, world!\n".as_ptr().cast()); }
    loop { core::hint::spin_loop(); }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
```

测试现象示意:

![pic](./pictures/03.png)

**test-2**

```rust
#![no_std]
#![no_main]
use oslab_user::syscall as sys;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    const NUM: usize = 20;
    const N_BUFFER: u32 = 8;
    let mut block_num = [0u32; NUM];
    let mut inode_num = [0u32; NUM];

    // SAFETY: 本例申请后归还对应编号, 每项只归还一次。
    unsafe {
        for i in 0..NUM {
            block_num[i] = sys::alloc_block() as u32;
        }

        sys::flush_buffer(N_BUFFER);
        sys::show_bitmap(0);

        for i in (0..NUM).step_by(2) {
            sys::free_block(block_num[i]);
        }

        sys::flush_buffer(N_BUFFER);
        sys::show_bitmap(0);

        for i in (1..NUM).step_by(2) {
            sys::free_block(block_num[i]);
        }

        sys::flush_buffer(N_BUFFER);
        sys::show_bitmap(0);

        for i in 0..NUM {
            inode_num[i] = sys::alloc_inode() as u32;
        }

        sys::flush_buffer(N_BUFFER);
        sys::show_bitmap(1);

        for i in 0..NUM {
            sys::free_inode(inode_num[i]);
        }

        sys::flush_buffer(N_BUFFER);
        sys::show_bitmap(1);
    }
    loop { core::hint::spin_loop(); }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
```

测试现象示意:

![pic](./pictures/04.png)

**test-3**

```rust
#![no_std]
#![no_main]
use oslab_user::syscall as sys;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    const PGSIZE: usize = 4096;
    const N_BUFFER: usize = 8;
    const BLOCK_BASE: u32 = 5000;

    // SAFETY: 两页堆内存由本例独占, 检查分配结果后才访问。
    // buffer令牌原样回传且不重复归还, 持有期间不调用fork或exit。
    unsafe {
        let top = sys::brk(0) as usize;
        if sys::brk(top + 2 * PGSIZE) != (top + 2 * PGSIZE) as isize {
            loop { core::hint::spin_loop(); }
        }
        let data = top as *mut u8;
        let tmp = (top + PGSIZE) as *mut u8;
        for i in 0..PGSIZE {
            data.add(i).write(0);
            tmp.add(i).write(0);
        }
        let mut buffer = [0usize; N_BUFFER];

        /*-------------一阶段测试: READ WRITE------------- */

        /* 准备字符串"ABCDEFGH" */
        for i in 0..8 {
            data.add(i).write(b'A' + i as u8);
        }
        data.add(8).write(b'\n');
        data.add(9).write(0);

        /* 查看此时的buffer_cache状态 */
        sys::print_str(c"\nstate-1 ".as_ptr().cast());
        sys::show_buffer();

        /* 向BLOCK_BASE写入字符 */
        buffer[0] = sys::get_block(BLOCK_BASE) as usize;
        sys::write_block(buffer[0], data);
        sys::put_block(buffer[0]);

        /* 查看此时的buffer_cache状态 */
        sys::print_str(c"\nstate-2 ".as_ptr().cast());
        sys::show_buffer();

        /* 清空内存副本, 确保后面从磁盘中重新读取 */
        sys::flush_buffer(N_BUFFER as u32);

        /* 读取BLOCK_BASE*/
        buffer[0] = sys::get_block(BLOCK_BASE) as usize;
        sys::read_block(buffer[0], tmp);
        sys::put_block(buffer[0]);

        /* 比较写入的字符串和读到的字符串 */
        sys::print_str(c"\n".as_ptr().cast());
        sys::print_str(c"write data: ".as_ptr().cast());
        sys::print_str(data);
        sys::print_str(c"read data: ".as_ptr().cast());
        sys::print_str(tmp);

        /* 查看此时的buffer_cache状态 */
        sys::print_str(c"\nstate-3 ".as_ptr().cast());
        sys::show_buffer();

        /*-------------二阶段测试: GET PUT FLUSH------------- */

        /* GET */
        buffer[0] = sys::get_block(BLOCK_BASE) as usize;
        buffer[3] = sys::get_block(BLOCK_BASE + 3) as usize;
        buffer[7] = sys::get_block(BLOCK_BASE + 7) as usize;
        buffer[2] = sys::get_block(BLOCK_BASE + 2) as usize;
        buffer[4] = sys::get_block(BLOCK_BASE + 4) as usize;

        /* 查看此时的buffer_cache状态 */
        sys::print_str(c"\nstate-4 ".as_ptr().cast());
        sys::show_buffer();

        /* PUT */
        sys::put_block(buffer[7]);
        sys::put_block(buffer[0]);
        sys::put_block(buffer[4]);

        /* 查看此时的buffer_cache状态 */
        sys::print_str(c"\nstate-5 ".as_ptr().cast());
        sys::show_buffer();

        /* FLUSH */
        sys::flush_buffer(3);

        /* 查看此时的buffer_cache状态 */
        sys::print_str(c"\nstate-6 ".as_ptr().cast());
        sys::show_buffer();

    }
    loop { core::hint::spin_loop(); }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
```
测试现象示意:

![pic](./pictures/05.png)

![pic](./pictures/06.png)

**尾声**

本次实验只是第三阶段的热身和铺垫~

我们引入了磁盘这种外设并具备了block-level的管理能力

在lab-8中, 我们要用inode将block组织起来并构建层次化的数据存储系统

我们即将进入真正的文件系统逻辑, 请你做好准备迎接新的挑战!

## 进阶目标

### 缓存策略

本次实验中，我们使用 LRU 管理缓冲块。如果连续读取一个较大的文件，原先经常使用的缓冲块会不会被淘汰？

请你尝试另一种缓存策略，与 LRU 进行比较。可以先构造重复读取少量磁盘块和顺序读取大量磁盘块两组测试，记录实际读盘次数。注意：仍在使用的缓冲块不能被回收。

### MBR/GPT

一块磁盘可以划分成多个分区，MBR和GPT就是记录分区位置和大小的两种格式。本实验在QEMU中直接使用磁盘镜像，在VisionFive2上使用固定位置的实验区，还没有通过分区表寻找文件系统。

请你尝试解析分区表，在块设备之上增加分区内读写接口。可以先区分扇区、磁盘块和分区偏移的单位，再用含多个分区的镜像观察各分区的起止位置，测试越界请求和无效表头，确认写入不会影响相邻分区。

### 异步 I/O

目前调用者提交磁盘请求后，会睡眠等待传输完成。异步I/O允许调用者先去做其他工作，等收到完成通知后再使用结果。等待期间，请求使用的数据页仍需留给设备。

请你在现有驱动之上设计非阻塞提交和完成通知，可以从请求句柄、队列满时的处理和数据页的使用期限入手。同时提交多个请求，比较等待时间和吞吐量，并观察取消请求时页面是否仍被设备访问。
