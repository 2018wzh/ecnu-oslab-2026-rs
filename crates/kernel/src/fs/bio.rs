//! 缓冲区缓存: 给块设备加上"缓存 + 锁 + 合并"。块设备的读写单位
//! 是扇区, 且很慢, 文件系统却会反复读同一个块 —— 这里把读过的块
//! 留在内存里, 并对同一个块的多次修改只在最后写回一次。
//!
//! 等待磁盘 I/O 要以毫秒计, 所以用睡眠锁而非自旋锁: 自旋锁在等待
//! 期间占着 CPU 不放, 单核上持锁者自旋等 I/O、而 I/O 完成中断需要
//! CPU, 必然死锁。被引用的缓冲区不能换出: 使用者手里还有指向它的
//! 裸指针, 换出会让那个指针指向别的块的数据。

use crate::sync::SleepLock;

// 固定槽位数: 静态数组让"下标 = 槽位"编译期成立, 不需要动态分配。
// 只需要"覆盖一个典型工作集", 不需要无限大。
/// 缓冲区缓存的槽位数。
pub const NBUF: usize = 30;

/// 块大小 (字节)。与块设备的扇区大小一致。
pub const BLOCK_SIZE: usize = 512;

// `data` 是裸字节数组而不是结构体: 一个缓冲区里装的可能是 inode、
// 目录项、位图或文件数据, 具体解释由使用者决定; 缓存层只负责搬字节。
/// 一个缓冲区。
#[repr(C, align(8))]
pub struct Buf {
    /// 这个缓冲区当前是哪个块 (块号)。
    pub blockno: usize,
    /// 有效数据的长度。
    pub valid: usize,
    /// 引用计数: >0 表示有人正在用它, 不能被复用。
    pub refcnt: usize,
    /// 数据 (按 8 字节对齐 —— 因为使用者可能会把它解释成
    /// 含 `u64` 字段的结构)。
    pub data: [u8; BLOCK_SIZE],
}

impl Buf {
    /// 一个空缓冲区。
    pub const fn empty() -> Self {
        Self {
            blockno: 0,
            valid: 0,
            refcnt: 0,
            data: [0; BLOCK_SIZE],
        }
    }
}

/// 缓冲区数组 + 保护它的睡眠锁。
pub struct BufCache {
    bufs: [Buf; NBUF],
    lock: SleepLock,
    /// 下一次从哪个槽开始找空闲缓冲区 (时钟算法的指针)。
    hand: usize,
}

impl BufCache {
    /// 新建一个空的缓存。
    pub const fn new() -> Self {
        Self {
            bufs: [const { Buf::empty() }; NBUF],
            lock: SleepLock::new(),
            hand: 0,
        }
    }

    // 用完必须调用 release, 否则缓冲区永远不能被复用 —— 症状是缓存
    // 逐渐失效, 因为是固定槽位, 最后所有块都要重新读, 但不会报错。
    /// 取得 `blockno` 对应的缓冲区, 并把引用计数加一。
    ///
    /// # Safety
    /// `dev` 必须是一个有效的块设备。
    pub unsafe fn bread(&mut self, dev: &mut dyn crate::fs::bio::BlockDevice, blockno: usize) -> usize { 0 }

    // 返回借用而非拷贝: 调用者通常要在数据上做解析, 拷贝 512 字节是浪费,
    // "用完就还"由借用检查器保证。检查 `valid`: bread 读失败时缓存可能
    // 是上次残留, 无效时返回空切片, 解析代码自然失败。
    /// 取一个缓冲区里的原始字节。
    pub fn block_data(&self, i: usize) -> &[u8] {
        if i >= NBUF || self.bufs[i].valid == 0 {
            return &[];
        }
        &self.bufs[i].data
    }

    /// 释放一个缓冲区的引用。
    pub fn release(&mut self, i: usize) {
        self.lock.lock();
        if i < NBUF && self.bufs[i].refcnt > 0 {
            self.bufs[i].refcnt -= 1;
        }
        self.lock.unlock();
    }

    /// 把缓冲区写回设备。
    ///
    /// # Safety
    /// `dev` 必须有效, 且 `i` 必须是一个被引用中的缓冲区。
    pub unsafe fn bwrite(&mut self, dev: &mut dyn BlockDevice, i: usize) -> bool { unimplemented!() }

    /// 当前有多少个缓冲区被引用 (自检用)。
    pub fn pinned(&self) -> usize {
        (0..NBUF).filter(|&i| self.bufs[i].refcnt > 0).count()
    }
}

impl Default for BufCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// TODO(lab-7): 实现缓冲区缓存 (本文件上半部分的 bread/release)。
//   注意淘汰策略: 被引用中的缓冲区不能被换出, 否则正在
//   读它的代码会拿到一块已经被别人改写的内存。
//   另外想清楚: 为什么这里必须用睡眠锁而不是自旋锁?
// 块设备接口
// ---------------------------------------------------------------------------
// 这个 trait 定义在这里而不是从 drivers re-export, 因为依赖方向是
// kernel -> drivers: kernel 不需要知道 virtio/SD 卡的存在。这里定义
// 的是文件系统需要的最小接口 (按块号读写), 由 drivers 去实现它。
// 注意 `oslab_drivers::block::BlockDevice` 的接口不同, 两个 trait 之间
// 的适配由 `adapter` 完成。
/// 文件系统需要的块设备接口。
pub trait BlockDevice {
    /// 读一个块。返回是否成功。
    ///
    /// # Safety
    /// `buf.len()` 必须等于 [`BLOCK_SIZE`]。
    unsafe fn read_block(&mut self, blockno: usize, buf: &mut [u8]) -> bool;

    /// 写一个块。返回是否成功。
    ///
    /// # Safety
    /// 同 [`BlockDevice::read_block`]。
    unsafe fn write_block(&mut self, blockno: usize, buf: &[u8]) -> bool;
}