//! 物理页分配器: 把 `[ALLOC_BEGIN, DRAM_END)` 切成 4KiB 页, 用空闲页
//! 链表管理。分成"内核池"与"用户池"两个池, 隔离内核页与用户页。

use core::sync::atomic::{AtomicUsize, Ordering};

use oslab_hal::arch;
use oslab_hal::platform;

/// 页大小 (字节) —— 直接复用 arch 层, 不在这里写死。
pub use oslab_hal::arch::mm::PAGE_SIZE;

/// 分配器管理的两个池。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    /// 内核页: 页表、内核栈、进程结构体等。
    Kernel,
    /// 用户页: 用户进程的地址空间与栈。
    User,
}

// 空闲页链表头: 每页的第一个字存下一页的物理地址, 0 表示结尾。
// 用原子 + CAS 而不加锁: 从核 lab-2 起就会并发分配, 而临界区只有
// 几条指令, 加锁开销反而比临界区还大。
static FREE_KERNEL: AtomicUsize = AtomicUsize::new(0);
static FREE_USER: AtomicUsize = AtomicUsize::new(0);

// 两个池各自的空闲页数与总页数 (只用于自检与打印)。
static KERNEL_FREE: AtomicUsize = AtomicUsize::new(0);
static KERNEL_TOTAL: AtomicUsize = AtomicUsize::new(0);
static USER_FREE: AtomicUsize = AtomicUsize::new(0);
static USER_TOTAL: AtomicUsize = AtomicUsize::new(0);

// 分配器是否已初始化。
static INITED: AtomicUsize = AtomicUsize::new(0);

/// 内核保留区大小: 从 `ALLOC_BEGIN` 起再留这么多字节给内核。
///
/// 用来容纳建页表所需的物理页、设备 MMIO 映射、per-CPU 结构等。
/// 不能太小: 映射 N 字节的 DRAM, 页表本身要花 N/512 字节, 这块必须
/// 从内核池里出。
pub const KERNEL_RESERVE: usize = 4 * 1024 * 1024;

// 链接脚本提供的符号: 内核镜像 (含栈) 之后的第一个可用地址。
unsafe extern "C" {
    static ALLOC_BEGIN: u8;
}

/// 初始化物理页分配器。
///
/// 由启动核调用**一次**: 它会重建整个空闲链表, 两个 hart 同跑会把
/// 链表串成两半。区间按 `kernel_reserve` 切成内核区与用户区。
pub fn pmem_init() { }

fn build_free_list(base: usize, pages: usize) -> usize { unimplemented!() }

/// 分配一页物理内存, 返回它的物理地址; 池空返回 0。
///
/// 返回 0 表示失败: 物理地址 0 在任何平台上都不是合法 DRAM 地址,
/// 是一个不可能被误用的哨兵值。分配出的页会被清零。
pub fn pmem_alloc(pool: Pool) -> usize { unimplemented!() }

/// 释放一页物理内存。
///
/// # Safety
///
/// * `pa` 必须来自 [`pmem_alloc`] 且尚未释放 (重复释放会让同一页被
///   分配给两个使用者, 造成随机内存损坏);
/// * 释放后不能再访问其内容, 第一个字已被用来存 `next` 指针;
/// * `pool` 必须与分配时的池一致。
pub unsafe fn pmem_free(pa: usize, pool: Pool) { }

/// 查询两个池的空闲/总页数 `(kernel_free, kernel_total, user_free, user_total)`。
///
/// 用于 lab-2 自检: "分配 N 页 -> 释放 N 页 -> 数字回到原值",
/// 能抓住链表串错、重复释放、忘减计数等静默错误。
pub fn pmem_stat() -> (usize, usize, usize, usize) {
    (
        KERNEL_FREE.load(Ordering::Relaxed),
        KERNEL_TOTAL.load(Ordering::Relaxed),
        USER_FREE.load(Ordering::Relaxed),
        USER_TOTAL.load(Ordering::Relaxed),
    )
}

/// 分配器是否已初始化。
pub fn is_inited() -> bool {
    INITED.load(Ordering::Acquire) != 0
}

/// 向上取整到 `align` 的倍数 (`align` 必须是 2 的幂)。
pub const fn align_up(v: usize, align: usize) -> usize {
    (v + align - 1) & !(align - 1)
}

/// 向下取整到 `align` 的倍数。
pub const fn align_down(v: usize, align: usize) -> usize {
    v & !(align - 1)
}