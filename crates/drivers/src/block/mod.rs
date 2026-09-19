//! 块设备 (`BlockDevice` trait), 运行期可有多个实现 (virtio-blk、SD 卡)。
//!
//! 块设备运行期可能有多个, 用 trait 多态; 上层文件系统只认识
//! [`BlockDevice::read`] / [`BlockDevice::write`], 不关心底层设备。
//! 方法返回 `Result` 强制调用者处理错误, 避免静默使用坏数据。

pub mod sdhci;
pub mod virtio_blk;

/// 扇区大小 (字节)。SD 卡与 virtio-blk 均支持 512 字节寻址, 固定成 512。
pub const SECTOR_SIZE: usize = 512;

/// 块设备的统一接口。
///
/// 方法签名用 `&mut self`, 让"同一时刻只有一个操作在进行"由借用检查器
/// 保证, 而不是靠内部加锁。多核下把设备放进 `Mutex<dyn BlockDevice>`。
pub trait BlockDevice: Send {
    /// 设备名 (用于日志)。
    fn name(&self) -> &'static str;

    /// 设备容量, 单位是扇区。
    ///
    /// 返回 0 表示"容量未知" (初始化未完成): 此时任何块号都越界, 所有
    /// I/O 明确失败, 而不是读到垃圾。
    fn capacity_sectors(&self) -> u64;

    /// 读 `count` 个扇区到 `buf`。
    ///
    /// `buf` 的长度至少为 `count * SECTOR_SIZE`。用切片而不是裸指针,
    /// 驱动必须检查长度, 否则 panic 而非静默越界。
    fn read(&mut self, block: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// 写 `count` 个扇区。
    fn write(&mut self, block: u64, buf: &[u8]) -> Result<(), BlockError>;

    /// 把缓冲区同步到介质 (刷缓存)。
    ///
    /// 默认空操作; SD 卡需要显式命令确认落盘, 会覆盖此实现。
    fn flush(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}

/// 块设备操作可能返回的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// 块号超出设备容量。
    OutOfRange,
    /// 缓冲区长度与请求的扇区数不匹配。
    BadBufferSize,
    /// 设备尚未初始化完成。
    NotReady,
    /// 设备报告了错误。
    DeviceError,
    /// 等待设备响应超时。
    Timeout,
    /// 该操作尚未实现 (区别于"实现错了")。
    Unsupported,
}

/// 一个块设备操作的重试策略。
///
/// SD 卡在真机会偶发失败: 明确的有界重试策略, 避免一次失败就挂掉
/// 或无限重试死循环。
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// 最多尝试几次 (含第一次)。
    pub attempts: usize,
    /// 每次重试之间忙等的时钟 tick 数。
    pub delay_ticks: u64,
}

impl RetryPolicy {
    /// 不重试。
    pub const NONE: Self = Self {
        attempts: 1,
        delay_ticks: 0,
    };
    /// SD 卡的推荐策略: 3 次尝试, 每次间隔 10000 tick。
    pub const SD: Self = Self {
        attempts: 3,
        delay_ticks: 10_000,
    };
}

/// 校验一次 I/O 请求的参数 (缓冲区长度、块号范围), 供各驱动复用。
pub fn check_request(dev_capacity: u64, block: u64, buf_len: usize) -> Result<usize, BlockError> {
    if buf_len == 0 || buf_len % SECTOR_SIZE != 0 {
        return Err(BlockError::BadBufferSize);
    }
    let count = (buf_len / SECTOR_SIZE) as u64;
    if dev_capacity == 0 {
        // 容量未知 -> 一切 I/O 都明确失败, 而不是读到垃圾。
        return Err(BlockError::NotReady);
    }
    if block
        .checked_add(count)
        .map_or(true, |end| end > dev_capacity)
    {
        // 用 `checked_add` 而不是 `block + count`: 后者在 block 接近
        // u64::MAX 时会回绕, 于是"越界检查"反而通过了 ——
        // 一个经典的整数溢出绕过边界检查的漏洞模式。
        return Err(BlockError::OutOfRange);
    }
    Ok(count as usize)
}

// ===========================================================================
// 按平台描述选择并初始化块设备
// ===========================================================================

/// 静态槽位: 全局唯一的块设备实例 (按平台描述二选一, 不需要堆)。
enum AnyBlock {
    /// QEMU 的 virtio-mmio 块设备。
    Virtio(super::block::virtio_blk::VirtioBlk),
    /// VisionFive2 的 DW MSHC (SDHCI) SD 卡控制器。
    Sdhci(super::block::sdhci::Sdhci),
}

impl AnyBlock {
    fn as_device(&mut self) -> &mut dyn BlockDevice {
        match self {
            AnyBlock::Virtio(d) => d,
            AnyBlock::Sdhci(d) => d,
        }
    }
}

static mut BLOCK_SLOT: Option<AnyBlock> = None;

/// virtqueue 的静态内存: 必须比驱动活得久且零初始化。
static mut VIRTIO_QUEUE: super::block::virtio_blk::VirtqueueMem =
    super::block::virtio_blk::VirtqueueMem::ZERO;

/// 按平台描述初始化块设备, 返回的引用在内核生命周期内有效。
///
/// # Safety
/// 必须只调用一次 (重复调用会覆盖槽位, 让先前的引用悬空)。
pub unsafe fn init_default(
    plat: &oslab_hal::platform::Platform,
) -> Result<&'static mut dyn BlockDevice, BlockError> {
    use oslab_hal::platform::BlockKind;

    let dev = match plat.block {
        BlockKind::VirtioMmio => {
            // SAFETY: 由调用者保证 virtio0_base 已映射, 且本函数只调用一次。
            let mut d = unsafe { super::block::virtio_blk::VirtioBlk::probe(plat.virtio0_base) }
                .ok_or(BlockError::NotReady)?;
            // SAFETY: 队列内存是静态的, 只在这里登记一次。
            unsafe { d.setup_queue(&raw mut VIRTIO_QUEUE) }?;
            AnyBlock::Virtio(d)
        }
        BlockKind::DesignWareMshc => {
            // SAFETY: 由调用者保证 sdhci_base 已映射。
            let mut d = unsafe { super::block::sdhci::Sdhci::new(plat) };
            d.init()?;
            AnyBlock::Sdhci(d)
        }
    };

    // SAFETY: 由调用者保证单次调用; 赋值后立即取引用, 中间没有别的访问。
    unsafe {
        (*core::ptr::addr_of_mut!(BLOCK_SLOT)) = Some(dev);
        let slot = (*core::ptr::addr_of_mut!(BLOCK_SLOT)).as_mut();
        Ok(slot.unwrap().as_device())
    }
}

// ===========================================================================
// 编译期自检
// ===========================================================================
const _: () = {
    // 扇区大小必须是 2 的幂, 否则"块号 -> 字节偏移"的移位计算不成立。
    assert!(SECTOR_SIZE.is_power_of_two());
    assert!(SECTOR_SIZE == 512);
};
