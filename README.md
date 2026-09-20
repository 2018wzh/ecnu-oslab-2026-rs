# LAB-1: 机器启动

## 1. 代码组织结构
```
ECNU-OSLAB-2026-RS
├── LICENSE        开源协议  
├── Cargo.toml     工作空间配置
├── rust-toolchain.toml  工具链配置
├── configs        平台与架构配置
├── xtask          构建、运行与镜像生成工具
├── pictures       README使用的图片目录  
├── README.md      实验指导书  
└── crates
    ├── hal/src    架构与平台接口
    │   ├── arch/riscv64
    │   │   ├── entry.S
    │   │   ├── boot.rs (TODO)
    │   │   ├── cpu.rs
    │   │   ├── csr.rs
    │   │   ├── sbi.rs
    │   │   └── kernel.ld.in  定义内核程序在链接时的布局
    │   └── platform  QEMU与VisionFive2平台参数
    ├── drivers/src/serial
    │   └── uart16550.rs
    └── kernel/src 内核源码
        ├── lock
        │   └── spinlock.rs (TODO)
        ├── console.rs
        ├── print.rs (TODO)
        ├── panic.rs
        └── main.rs (TODO)
```
## 2. 实验核心目标

完成多核的机器启动, 进入kernel_main函数并输出启动信息 (下图为双核示意)  

![alt text](pictures/01.png)

QEMU使用双核，VisionFive2使用四核，每个CPU输出一条启动信息

## 3. 具体任务

### 3.1 机器启动本身

要想实现上述核心目标，仔细想想只需要完成两件事

1. 让内核在QEMU或VisionFive2上跑起来（分别为双核、四核启动）：**entry.S** 到 **boot.rs** 到 **main.rs**  

2. 让内核向屏幕输出一些字符串，也就是实现内核中的`print!()`

第一件事需要你研究一下xv6的启动流程，只需要看到进入 **main.c** 就够了，对应本实验的 **main.rs**

与xv6的启动流程相比，本实验有一个不同之处：进入S-mode之前的工作已经由OpenSBI完成了。在QEMU上，我们直接通过OpenSBI启动；在VisionFive2上，则由U-Boot配合OpenSBI完成启动。

接下来需要你完成的是：让主核做好初始化，再启动其他核，让它们进入kernel_main函数。这里要注意，主核不一定是hart 0，可以通过代码中提供的接口判断。内核使用从0开始的CPU编号，而VisionFive2的硬件hart编号是1～4，二者不要混淆。

第二件事需要你先阅读一下**crates/drivers/src/serial/uart16550.rs**，里面包括串口（最基本的字符输入输出设备）驱动

读完之后你需要通过`console::putc`完成**crates/kernel/src/print.rs**中的`Writer::write_str`和`print`。`core::fmt`已经提供了格式化功能，但一次打印可能要分几次输出字符，因此应在开始打印前上锁，等所有字符都输出完再解锁

完成本章任务后，可以用以下命令构建和运行：

```bash
cargo xtask build --config riscv64-qemu-virt
cargo xtask run --config riscv64-qemu-virt
cargo xtask image --config riscv64-visionfive2
```

将`target/riscv64-visionfive2/kernel.itb`复制到开发板TF卡的FAT分区，在U-Boot中加载：

```text
fatload mmc 1:1 ${kernel_addr_r} kernel.itb
bootm ${kernel_addr_r}
```

其中`mmc 1:1`按实际设备与分区调整。

### 3.2 print!面临的资源竞争问题

串口是一种设备资源, `print!()`利用它输出字符本质是在一段时间内持有这种资源

例如, 输出`"hello,world!"`其实是连续占用串口资源12次, 调用12次`console::putc()`

假设同时存在第二个`print!()`执行流要打印`"hello,os!"`, 它就会与执行流1形成竞争关系

两条执行流交错带来的输出可能包括:

```
# 混乱的情况
hellohello,,world!os!
hheelllloo,,wosrld!!
hhello,world!ello,os!
......
# 有序的情况
hello,world!hello,os!
hello,os!hello,world!
```

我们需要一种手段, 保证`print!()`过程中, UART资源始终只被一个执行流占有同时不可抢占

生活中的例子: 公共卫生间通过"门锁"来保证马桶这一资源在一段时间内只被一人独占

映射到操作系统, 最简单的"资源锁"就是“自旋锁”, 它的实现位于**crates/kernel/src/lock/spinlock.rs**

```rust
// 在print!中使用自旋锁的方法
use crate::{console, lock::SpinLock};

static LK: SpinLock = SpinLock::UNINIT;

// 锁的初始化
// SAFETY: 主核在其他执行流使用此锁前初始化一次。
unsafe { LK.init(); }

// 上锁
let guard = LK.lock();

// 独占资源
console::putc(b'O');
console::putc(b'S');

// 解锁，也可以由守卫离开作用域时自动完成
drop(guard);
```

自旋锁的可靠性依赖**开关中断**和**原子操作**这两个关键概念，你需要完全理解

- 上锁前关闭本CPU的中断，可以避免中断处理再次请求当前CPU已持有的锁；解锁后恢复之前的中断状态

- 原子操作可以保证多CPU的情况下并行执行流不会同时上锁成功

完成上述工作后，你应当可以实现图片所示的效果 (在**main.rs**的合适位置输出每个CPU的启动信息)  

## 4. 课后实验

这里有两个额外的实验帮助你理解锁的用处 

### 4.1 并行加法

在完成本章启动、打印和锁的任务后，临时替换主函数测试。Rust用分开的原子load和store避免数据竞争，但“读取、加一、写回”整体并不原子，仍然可能丢失更新。

```rust
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::{Acquire, Release, Relaxed}};
use oslab_hal::{arch::cpu, platform::NCPU};

static STARTED: AtomicBool = AtomicBool::new(false);
static SUM: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    let cpuid = cpu::cpu_id();
    if cpu::is_boot_cpu() {
        crate::print::init();
        crate::println!("cpu {} is booting!", cpuid);
        for id in 0..NCPU {
            if id != cpuid { cpu::start_cpu(id).expect("start_cpu failed"); }
        }   
        STARTED.store(true, Release);
        for _ in 0..1000000 {
            let value = SUM.load(Relaxed);
            SUM.store(value + 1, Relaxed);
        }   
        crate::println!("cpu {} report: sum = {}", cpuid, SUM.load(Relaxed));
    } else {
        while !STARTED.load(Acquire) { core::hint::spin_loop(); }
        crate::println!("cpu {} is booting!", cpuid);
        for _ in 0..1000000 {
            let value = SUM.load(Relaxed);
            SUM.store(value + 1, Relaxed);
        }   
        crate::println!("cpu {} report: sum = {}", cpuid, SUM.load(Relaxed));
    }  
    cpu::park()
}
```

在 **main.rs** 中测试上述代码，所有CPU完成正常累加后的总数应为QEMU双核的`2000000`或VisionFive2四核的`4000000`；report输出的是各核读取时的值，打印顺序不代表完成顺序

但是未加锁的双核输出可以用下面的例子说明  

```
cpu 0 is booting!
cpu 1 is booting!
cpu 0 report: sum = 1128497
cpu 1 report: sum = 1143332
```

考虑如何使用锁进行修正，修正后的双核输出可能是这样的  

```
cpu 0 is booting!
cpu 1 is booting!
cpu 0 report: sum = 1996573
cpu 1 report: sum = 2000000
```

简单说明上锁和解锁的位置不同会有什么影响（tips: 锁的粒度粗细）

### 4.2 并行输出  

尝试去掉`print!`里的锁，参考4.1的实验思路，设计测试方法使得`print!`的输出出现交错的情况  

4.1和4.2的测试代码和实验结果可以附在你的README中, 但是不要体现在你的代码里

## 5. 关于代码仓库的维护

1. 每次实验需要在上次实验的基础上继续往下做，假设教师仓库已配置为`upstream`

    首次开始lab-1时，使用`git fetch upstream`和`git checkout -b lab-1 upstream/lab-1`获取并切换到实验分支；已有lab-1分支时直接切换

    完成lab-1并提交自己的实现后，若个人提交基于`upstream/lab-1`且尚无lab-2分支，可用以下命令进入下一次实验：

    ```bash
    git fetch upstream
    git checkout -b lab-2 lab-1
    git rebase --onto upstream/lab-2 upstream/lab-1
    ```

    解决冲突时保留自己的实现并接入新框架，你对lab-2的修改不会影响lab-1

    以此类推，当你从lab-1开始走到lab-9时，你会获得越来越完整和强大的内核  

2. 你的代码仓库应该由 **代码 + Markdown文档** 两部分构成  

    文档内容不做明确要求，你有很高的自由度决定写什么和写多少

    提供一些建议: 
    
    - 本次实验新增了哪些功能，实现了什么效果

    - 对本次实验中某个过程的理解和思考

    - 本次实验和之前的实验构成什么样的逻辑联系

    - 本次实验花费的时间, 你和队友的贡献分别是什么

    - 可以使用markdown的分层分点来增加条理性，便于别人阅读和抓住重点

    **总之，这是你的代码仓库，请对你自己的代码和文档负责**  
    
    **注意，代码是继承和连续发展的, 但文档不是，每次的文档都是全新一页**  

3. 提醒: 之所以要求大家维护代码仓库，是为了查看大家的提交记录

    所以请及时同步当天写的代码到线上仓库，不要攒到最后一口气提交，否则可能被误判为不当行为

## 进阶目标

### UEFI

内核开始运行之前，需要有人把它装入内存，并把控制权交给它。UEFI定义了一套固件与操作系统之间的接口，启动程序可以借助它读取文件、申请内存、获取内存布局。利用这些服务，我们可以尝试另一条装载内核的路径。

请你尝试编写一个UEFI启动程序，装载并进入内核。可以先以输出启动信息为目标，梳理固件把控制权交给内核的过程，再检查退出启动服务后，内核是否还依赖这些已经不能使用的服务。

### DTB 动态发现

本次实验把CPU数量、内存范围和串口地址等信息写在平台配置里，换一种硬件配置就可能需要修改代码。设备树可以把这些硬件信息组织起来，DTB就是它编译后的二进制形式。内核读取启动时传入的DTB，就有机会了解当前机器，而不必把所有参数写死。

请你尝试从DTB中读取CPU、内存和设备信息，先与现有平台配置比较，再改变QEMU的CPU数量或内存大小，观察内核能否识别变化。注意：冷启动传入的DTB与从核启动参数不是一回事，固件保留的内存也不能交给内核分配。

### BootInfo

如果内核既支持设备树，又支持其他启动方式，是否每个模块都要了解它们各自的信息格式？可以在启动代码和内核之间约定一个`BootInfo`结构，只记录内核需要的CPU、内存和设备信息。它不是另一种设备树格式，而是由我们自己设计的统一接口，启动代码负责把不同来源的信息整理进去。

请你先梳理当前启动路径需要传递哪些信息，再尝试用`BootInfo`把它们交给内核。观察内核是否还需要区分这些信息的来源，并检查启动时的临时数据不再使用后，`BootInfo`中的内容是否仍然有效。
