# LAB-2: 内存管理初步

**前言**

在lab-1中, 我们学习了机器启动流程、UART设备驱动、格式化输出和自旋锁

完成lab-1后, OS内核已经可以进入kernel_main函数并掌控UART资源做一些输出了

在lab-2中, 我们要开始认识和管理“程序除了CPU外最常访问的共享资源——内存”

内存管理的实现不是一步到位的, lab-2主要关注物理内存和内核态虚拟内存, 剩余部分将在后面的实验逐渐完善

## 代码组织结构
```
ECNU-OSLAB-2026-RS
├── Cargo.toml     工作空间配置
├── configs        平台与架构配置
├── xtask          构建、运行与镜像生成工具
├── pictures       README使用的图片目录 (CHANGE)
├── README.md      实验指导书 (CHANGE)
└── crates
    ├── hal/src
    │   ├── arch/riscv64
    │   │   ├── kernel.ld.in (CHANGE, 增加内存边界标记)
    │   │   └── mm.rs (NEW, 页表类型与切换接口)
    │   └── platform (CHANGE, 增加PLIC范围)
    └── kernel/src
        ├── mem
        │   ├── mod.rs (NEW)
        │   ├── pmem.rs (TODO, 物理内存管理)
        │   └── kvm.rs (TODO, 内核态虚拟内存管理)
        └── main.rs (TODO)
```
**标记说明**

**NEW**: 新增源文件, 直接拷贝即可, 无需修改

**CHANGE**: 旧的源文件发生了更新, 直接拷贝即可, 无需修改

**TODO**: 你需要实现新功能 / 你需要完善旧功能

## 第一阶段: 物理内存

首先需要关注的文件是 **crates/hal/src/arch/riscv64/kernel.ld.in** 文件, 它规定了内核文件 **oslab-kernel** 在载入内存时的布局

内核使用的物理内存按照地址空间划分为三个部分：

- **装载地址 ~ rodata_end** 存放了 **oslab-kernel的代码和只读数据**

- **rodata_end ~ kernel_end** 存放了 **oslab-kernel的数据**

- **kernel_end ~ DRAM_BASE + DRAM_SIZE** 属于 **未使用的可分配的物理页**

链接脚本中的`@LOAD@`来自平台配置，QEMU为`0x80200000`，VisionFive2为`0x40200000`。两平台本章都使用128MB内存范围，`DRAM_BASE`分别为`0x80000000`和`0x40000000`；内核之前的区域留给固件，不参与分配。

如果你想深入了解可执行文件(ELF)的布局信息, 可以自行查阅资料, 在lab-9中我们会再提

前两个区域的物理页会一直被内核占用, 不会纳入动态分配和回收的范围, 需要管理的只有第三个区域的物理页

首先介绍物理内存管理的基本原理: **4KB物理页切分 + 空闲链表组织**

**kernel_end ~ DRAM_BASE + DRAM_SIZE** 这块物理空间被切分为N个4KB物理页(不会有剩余)

此外, 为了分别管理内核页和普通数据页, 我们设置了两个`Region`, 基于**KERNEL_PAGES**进行边界划分；前1024页供内核使用，其余页面供普通数据使用

`KERN_REGION` 记录了内核空间的空闲物理页情况, `USER_REGION` 记录了普通数据页的空闲情况

`Region` 描述了一组空闲页链表, 包括起止位置、空闲页面数量、链表头节点、保证一致性的锁

下面的图片显示了物理页的申请和释放在链表上是如何体现的

![pic](./pictures/01.png)

接下来讨论物理内存管理的函数实现:

```
pub fn init();    // 初始化系统, 只调用一次
pub fn alloc(kernel: bool) -> usize;  // 申请一个空闲的物理页，返回地址
pub unsafe fn free(pa: usize, kernel: bool);    // 释放一个之前申请的物理页
```

这三个函数体现了经典的共享资源管理方法：初始化共享资源, 占有共享资源, 释放共享资源

`core::ptr`提供了`write_bytes`等辅助函数, 能简化一些操作

为了保证资源共享的可靠性, 当尝试访问`Region`时需要获取和释放自旋锁

## 第一阶段: 测试用例

**test-1**

完成前面的任务后，在`crates/kernel/src/main.rs`中临时替换主函数测试。QEMU使用双核，VisionFive2使用四核，各核分配完毕后再一起释放页面。

```rust
use core::sync::atomic::{AtomicBool, Ordering::{Acquire, Release}};
use oslab_hal::{arch::cpu, platform::NCPU};
use crate::mem::{pmem, PAGE_SIZE};

static STARTED: AtomicBool = AtomicBool::new(false);
static OVER: [AtomicBool; NCPU] = [const { AtomicBool::new(false) }; NCPU];

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    let cpuid = cpu::cpu_id();

    if cpu::is_boot_cpu() {
        crate::print::init();
        pmem::init();
        for id in 0..NCPU {
            if id != cpuid { cpu::start_cpu(id).expect("start_cpu failed"); }
        }
        STARTED.store(true, Release);
    } else {
        while !STARTED.load(Acquire) { core::hint::spin_loop(); }
    }
    crate::println!("cpu {} is booting!", cpuid);

    let mut mem = [0usize; pmem::KERNEL_PAGES / NCPU];
    for page in &mut mem {
        *page = pmem::alloc(true);
        // SAFETY: 本核独占刚申请的页面。
        unsafe {
            core::ptr::write_bytes(*page as *mut u8, 1, PAGE_SIZE);
            crate::println!("mem = {:#x}, data = {}", *page, *(*page as *const i32));
        }
    }
    crate::println!("cpu {} alloc over", cpuid);
    OVER[cpuid].store(true, Release);

    for over in &OVER {
        while !over.load(Acquire) { core::hint::spin_loop(); }
    }

    for page in mem {
        // SAFETY: 本核已停止使用页面，也没有其他核引用它。
        unsafe { pmem::free(page, true); }
    }
    crate::println!("cpu {} free over", cpuid);
    cpu::park()
}
```

这个测试用例的作用是：

1. 各CPU并行申请内核池的全部物理页, 赋值并输出信息

2. 待申请全部结束, 并行释放所有申请的物理内存

下面是并行申请和释放的输出示意：

![pic](./pictures/02.png)

**test-2**

下面两个函数放在`crates/kernel/src/mem/pmem.rs`中，以便检查模块内部的`USER_REGION`。每次只运行一个测试，由主核在初始化后调用，其他核暂不分配内存；第一个测试会因内存耗尽而停止。

```rust
/*--------------------------------- 测试代码 ----------------------------------*/
use super::PAGE_SIZE;

// 测试目标：耗尽内核/用户区域内存
pub fn test_case_1() {
    loop {
        let _page = alloc(true);
        // let _page = alloc(false);
    }
}

const TEST_CNT: usize = 10;

// 测试目标: 常规申请和释放操作
pub fn test_case_2() {
    let user_ar = &raw mut USER_REGION;
    let mut user_pages = [0usize; TEST_CNT];

    // SAFETY: 内存池已经初始化；本测试由主核单独执行。
    // 检查计数与链表时持有池锁，访问的数据页由本测试独占。
    unsafe {
        crate::println!("=== test_case_2: Phase 1 - Allocate User Pages ===");
        for i in 0..TEST_CNT {
            user_pages[i] = alloc(false);

            crate::println!("Allocated user page[{}] @ {:#x}", i, user_pages[i]);

            if !(user_pages[i] >= (*user_ar).begin && user_pages[i] < (*user_ar).end) {
                crate::println!("Assertion failed: Page address out of bounds! Page: {:#x}, Region: [{:#x}, {:#x})",
                    user_pages[i], (*user_ar).begin, (*user_ar).end);
                panic!("Page address out of user region bounds");
            }

            core::ptr::write_bytes(user_pages[i] as *mut u8, 0xAA, PAGE_SIZE);
        }

        crate::println!("=== test_case_2: Phase 2 - Pre-free Check ===");
        let guard = (*user_ar).lk.lock();
        let expected_before = ((*user_ar).end - (*user_ar).begin) / PAGE_SIZE - TEST_CNT;
        let actual = (*user_ar).allocable as usize;
        crate::println!("Expected allocable: {}, Actual: {}", expected_before, actual);
        assert_eq!(actual, expected_before, "Allocable count incorrect before free");
        drop(guard);

        crate::println!("=== test_case_2: Phase 3 - Free Pages ===");
        for i in 0..TEST_CNT {
            free(user_pages[i], false);
            crate::println!("Free user page[{}] @ {:#x}", i, user_pages[i]);
        }

        crate::println!("=== test_case_2: Phase 4 - Post-free Check ===");
        let guard = (*user_ar).lk.lock();
        let expected_after = ((*user_ar).end - (*user_ar).begin) / PAGE_SIZE;
        let actual = (*user_ar).allocable as usize;
        crate::println!("Expected allocable: {}, Actual: {}", expected_after, actual);
        assert_eq!(actual, expected_after, "Allocable count not restored after free");
        if (*user_ar).list_head.next != 0 {
            crate::println!("Free list head @ {:#x}", (*user_ar).list_head.next);
        } else {
            panic!("Free list is empty after freeing pages");
        }
        drop(guard);

        crate::println!("=== test_case_2: Phase 5 - Reallocate & Verify Zero ===");
        for i in 0..TEST_CNT {
            let page = alloc(false);
            crate::println!("Reallocated page[{}] @ {:#x}", i, page);

            let mut non_zero = false;
            for j in 0..PAGE_SIZE / core::mem::size_of::<i32>() {
                let value = *((page as *const i32).add(j));
                if value != 0 {
                    non_zero = true;
                    crate::println!("Non-zero value detected at offset {}: {:#x}", j, value);
                    break;
                }
            }
            assert!(!non_zero, "Memory not zeroed on allocation");
            crate::println!("Zero verification passed");
        }

        crate::println!("test_case_2 passed!");
    }
}
```

这个测试用例的作用是：

1. 测试内存耗尽的`panic!`是否正常触发

2. 测试用户空间物理页申请和释放的正确性

## 第二阶段: 内核态虚拟内存

完成物理内存管理的部分后, 你应该注意到“内存”和“串口”这两种共享资源的区别:

**串口资源是没有区别的, 而内存资源被细分为很多个通过"地址"来区分的4KB物理页**

- 因此, 我们需要一种机制来记录每个程序获得了哪些4KB物理页面

- 此外, 考虑到内存编程模型的灵活性和通用性, 我们需要给各个应用程序提供“独占内存资源”的幻觉

为了实现这两个目的, 我们引入**虚拟内存**这一重要概念

简单来说, 我们要建立一个表格, 用于记录虚拟地址空间到物理地址空间的对应关系, 并通过MMU自动完成翻译

**crates/hal/src/arch/riscv64/mm.rs** 中的注释介绍了虚拟内存的一种规范**SV39**, 即39 bit虚拟地址的虚拟内存

之所以要遵守这个规范, 是为了能在RISC-V体系结构的机器上正常使用MMU,  你可以查看手册获得更多信息

虚拟内存的构建围绕两个核心概念：**页表项(PTE)** 和 **页表(pgtbl)**

页表是由页表项构成的, 你可以理解成数组和数组里元素的关系

一个页表项对应一个物理页, 页表项主要由两部分组成：

- 它所管理的物理页的**页号** (PPN字段)

- 它所管理的物理页的**标志位** (低10bit)

**提示:** 页表本身也是存放在物理页中, 指向下一级页表的有效PTE中`R W X`都是0

这并不表示页表所在的物理页不能读写，而是告诉MMU继续查找下一级页表

下面的图片显示了页表的示意图和实际状态:

![pic](./pictures/03.png)

页表(比如`kvm::ROOT`)刚刚初始化时只是一个被清空的4KB物理页

随着`mmap`操作的增加, 页表开始伸展出去, 直至完全长成一个能管理512GB虚拟地址空间的树；本实验只使用其中的低半区，地址须小于`VA_MAX`

关于页表的三级组织结构:

- 能从**顶级页表**的PTE里获得**次级页表**所在的物理页的物理页号和标志位

- 能从**次级页表**的PTE里获得**低级页表**所在的物理页的物理页号和标志位

- 能从**低级页表**的PTE里获得**一般物理页**(真正存储数据和代码)的物理页号和标志位

到此为止, 你应该对页表和页表项建立起基本的认识了: 页表是存储分级页表项的树形结构

介绍完背景之后简单说明你需要做的事情, **kvm.rs**中的函数推荐按照以下顺序去实现

`kvm::getpte -> kvm::mappages -> kvm::unmappages`

**提示:** 实现过程中可以使用 **crates/hal/src/arch/riscv64/mm.rs** 中的常量和函数

**核心:** 理解页表的构成(页表项与三级映射)和页表操作(映射与解映射)

我们提供了一个`kvm::print`函数, 它可以输出页表中所有有用信息, 可以用于Debug

完成基本的页表操作函数后, 我们需要给内核页表 **kvm::ROOT**设置映射关系并为每个CPU启用它

`kvm::init -> kvm::init_hart`

**kvm::ROOT** 的映射大致可以划分成两部分:

- 硬件寄存器区域, 这部分地址空间不能分配回收只能读写, 通过这些地址可以访问QEMU或开发板上的设备

- 可用内存区域, 即装载地址到`DRAM_BASE + DRAM_SIZE`，不包含内核之前的固件保留区

内核页表对这两部分的映射都是**虚拟地址等于物理地址**的直接映射, 未来的用户页表则不同

映射完毕后我们的**kvm::ROOT**就可以上线工作了, 把根页表的物理页号和Sv39模式写入**satp**寄存器正式开启**MMU**翻译

我们终于结束了直接访问物理地址(`satp = 0`)的时代 (虽然目前物理地址恰好等于虚拟地址)

之后的内存访问本质都是访问虚拟地址, 虚拟地址经过页表和MMU的协作, 被自动翻译为物理地址

## 第二阶段: 测试用例

**test-1**

在`crates/kernel/src/main.rs`中临时替换主函数测试。本例只让主核检查页表；正常启动时，其他核也需要在内核页表初始化完成后调用`kvm::init_hart`。

```rust
use oslab_hal::arch::cpu;
use crate::mem::{pmem, kvm, *};

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    let cpuid = cpu::cpu_id();

    if cpu::is_boot_cpu() {
        crate::print::init();
        pmem::init();
        kvm::init();
        kvm::init_hart();

        crate::println!("cpu {} is booting!", cpuid);

        let test_pgtbl = pmem::alloc(true) as PageTable;
        let mut mem = [0usize; 5];
        for page in &mut mem {
            *page = pmem::alloc(false);
        }

        // SAFETY: 主核独占测试页表和数据页；其他核尚未启动。
        unsafe {
            crate::println!("\ntest-1\n");
            kvm::mappages(test_pgtbl, 0, mem[0], PAGE_SIZE, R);
            kvm::mappages(test_pgtbl, PAGE_SIZE * 10, mem[1], PAGE_SIZE / 2, R | W);
            kvm::mappages(test_pgtbl, PAGE_SIZE * 512, mem[2], PAGE_SIZE - 1, R | X);
            kvm::mappages(test_pgtbl, PAGE_SIZE * 512 * 512, mem[3], PAGE_SIZE, R | X);
            kvm::mappages(test_pgtbl, VA_MAX - PAGE_SIZE, mem[4], PAGE_SIZE, R | W);
            kvm::print(test_pgtbl);

            crate::println!("\ntest-2\n");
            kvm::mappages(test_pgtbl, 0, mem[0], PAGE_SIZE, R | W);
            kvm::unmappages(test_pgtbl, PAGE_SIZE * 10, PAGE_SIZE, true);
            kvm::unmappages(test_pgtbl, PAGE_SIZE * 512, PAGE_SIZE, true);
            kvm::print(test_pgtbl);
        }
    }
    cpu::park()
}
```

这个测试用例测试了两件事情:

1. 使用内核页表后你的OS内核是否还能正常执行

2. 使用映射和解映射操作修改你的页表, 使用kvm::print输出它被修改前后的对比

下面是页表输出的示意:

![alt text](./pictures/04.png)

**test-2**

将下面的函数放在`crates/kernel/src/main.rs`中，由主核在`pmem::init`和`kvm::init`完成后调用。导入的模块与上例相同。

```rust
/*---------------------------------- 测试代码 --------------------------------*/

fn test_mapping_and_unmapping() {
    // 1. 初始化测试页表
    let pgtbl = pmem::alloc(true) as PageTable;

    // SAFETY: 本测试独占页表和数据页，没有其他映射使用这些数据页。
    unsafe {
        core::ptr::write_bytes(pgtbl as *mut u8, 0, PAGE_SIZE);

        // 2. 准备测试条件
        let va_1 = 0x100000;
        let va_2 = 0x8000;
        let pa_1 = pmem::alloc(false);
        let pa_2 = pmem::alloc(false);

        // 3. 建立映射
        kvm::mappages(pgtbl, va_1, pa_1, PAGE_SIZE, R | W);
        kvm::mappages(pgtbl, va_2, pa_2, PAGE_SIZE, R);

        // 4. 验证映射结果
        let pte = kvm::getpte(pgtbl, va_1, false).expect("test_mapping_and_unmapping: pte_1 not found");
        assert_ne!(*pte & V, 0, "test_mapping_and_unmapping: pte_1 not valid");
        assert_eq!(pte_to_pa(*pte), pa_1, "test_mapping_and_unmapping: pa_1 mismatch");
        assert_eq!(*pte & (R | W), R | W, "test_mapping_and_unmapping: flag_1 mismatch");

        let pte = kvm::getpte(pgtbl, va_2, false).expect("test_mapping_and_unmapping: pte_2 not found");
        assert_ne!(*pte & V, 0, "test_mapping_and_unmapping: pte_2 not valid");
        assert_eq!(pte_to_pa(*pte), pa_2, "test_mapping_and_unmapping: pa_2 mismatch");
        assert_eq!(*pte & R, R, "test_mapping_and_unmapping: flag_2 mismatch");

        // 5. 解除映射
        kvm::unmappages(pgtbl, va_1, PAGE_SIZE, true);
        kvm::unmappages(pgtbl, va_2, PAGE_SIZE, true);

        // 6. 验证解除映射结果
        let pte = kvm::getpte(pgtbl, va_1, false).expect("test_mapping_and_unmapping: pte_1 not found");
        assert_eq!(*pte & V, 0, "test_mapping_and_unmapping: pte_1 still valid");
        let pte = kvm::getpte(pgtbl, va_2, false).expect("test_mapping_and_unmapping: pte_2 not found");
        assert_eq!(*pte & V, 0, "test_mapping_and_unmapping: pte_2 still valid");

        // 7. 由于页表的释放函数还没实现, 作为测试用例可以展示不释放页表空间
    }
    crate::println!("test_mapping_and_unmapping passed!");
}
```

这个测试用例主要关注映射和解映射是否正确执行

下面是映射和解映射测试的输出示意:

![pic](./pictures/05.png)

**补充更多测试用例**

因为你未来会依赖现在写的这些函数, 如果现在没发现隐藏的错误, 未来的Debug会更困难

所以每个模块写完后都要进行尽可能完善的测试, 助教提供的测试用例远远不够, 请对你的代码负责

另外, 值得强调的一点是：学会使用`panic!`和`assert!`做必要的检查

在出问题前输出有价值的错误信息, 比系统直接卡死或进入错误状态, 更容易Debug

这种理论又叫**防御性编程**, 对输入参数保持警惕, 充分检查, 确保错误不会在函数间传递

**尾声**

这次实验在`kvm::init`里埋下了一些伏笔: PLIC的寄存器映射还没用起来，CLINT由OpenSBI管理

不要着急, 下一次实验的主题是——**中断和异常**, 那时会用到

实验的基本原则之一: 绝大多数增添或修改只服务于本次的实验目标, 少量服务于下一次实验的实验目标

## 进阶目标

### buddy 分配器

本次实验每次只分配一页。如果需要一块连续的多页内存，空闲链表还能方便地找到它吗？buddy分配器按2的幂次管理页块，需要时把大块拆成小块，释放时尝试将相邻的伙伴合并。

请你尝试用buddy分配器管理物理页。可以先从单核下的拆分和合并开始，再加入并发测试；反复申请不同大小的页块并全部释放，观察空闲容量是否恢复，比较它与单页链表的内存利用情况。

### 内核堆 alloc

如果一个对象只需要几十字节，为它分配整个物理页就有些浪费了。内核堆可以向页分配器申请页面，再把页面中的小块内存交给这些对象使用。

请你尝试实现一个内核堆，可以先支持几种固定大小的对象，再考虑不同的大小和对齐要求，并尝试接入Rust的分配接口。反复申请和释放这些对象，观察实际占用了多少物理页，并检查内存耗尽时能否正确处理。

### 大页

本次实验的映射都使用4KB页面。Sv39还允许在较高层的页表中直接记录映射，使用2MiB或1GiB的大页，这样可以减少映射大块连续内存时需要的页表页。

请你尝试加入大页映射，可以先选择一段满足大小和地址对齐要求的内存。比较使用大页前后的页表页数量，再考虑只解除其中一小段映射时该怎样处理；遍历页表时，要能区分大页映射与指向下一级页表的条目。
