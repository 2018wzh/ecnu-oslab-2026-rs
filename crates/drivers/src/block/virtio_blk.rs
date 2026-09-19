//! QEMU 平台的 VirtIO-MMIO 块设备驱动。
//!
//! VirtIO 是半虚拟化设备: 驱动与 QEMU 通过共享内存 (virtqueue) 通信,
//! 一次 I/O 只需要往共享内存里放一个描述符并敲 doorbell。本驱动实现
//! [`BlockDevice`], 上层文件系统通过 trait 调用, 不知道下面是 VirtIO。
//!
//! 用轮询而非中断 (简单、不依赖 PLIC、出问题表现为超时而非挂起), 代价
//! 是等待时空转。

use oslab_hal::platform::Platform;

use super::{BlockDevice, BlockError};
use crate::mmio::Mmio;

// ---------------------------------------------------------------------------
// VirtIO-MMIO 寄存器偏移 (virtio 1.x 规范第 4.2.2 节)
// ---------------------------------------------------------------------------

/// 魔数, 固定为 0x74726976 (ASCII "virt")。
pub const MAGIC: u32 = 0x7472_6976;
/// 设备版本: legacy。
pub const VERSION_LEGACY: u32 = 1;
/// 设备版本: modern。
pub const VERSION_MODERN: u32 = 2;
/// 块设备的 device-id。
pub const DEVICE_ID_BLOCK: u32 = 2;

// 完整寄存器表: 当前阶段只有探测路径用了其中一部分, 其余是 lab-7 实现
// virtqueue 时需要的。用 `#[allow(dead_code)]` 标注"这是有意的"。
#[allow(dead_code)]
mod regs {
    pub const REG_MAGIC: usize = 0x000;
    pub const REG_VERSION: usize = 0x004;
    pub const REG_DEVICE_ID: usize = 0x008;
    pub const REG_VENDOR_ID: usize = 0x00c;
    pub const REG_DEVICE_FEATURES: usize = 0x010;
    pub const REG_DEVICE_FEATURES_SEL: usize = 0x014;
    pub const REG_DRIVER_FEATURES: usize = 0x020;
    pub const REG_DRIVER_FEATURES_SEL: usize = 0x024;
    pub const REG_QUEUE_SEL: usize = 0x030;
    pub const REG_QUEUE_NUM_MAX: usize = 0x034;
    pub const REG_QUEUE_NUM: usize = 0x038;
    pub const REG_QUEUE_READY: usize = 0x044;
    pub const REG_QUEUE_NOTIFY: usize = 0x050;
    pub const REG_INTERRUPT_STATUS: usize = 0x060;
    pub const REG_INTERRUPT_ACK: usize = 0x064;
    pub const REG_STATUS: usize = 0x070;
    pub const REG_QUEUE_DESC_LOW: usize = 0x080;
    pub const REG_QUEUE_DESC_HIGH: usize = 0x084;
    pub const REG_QUEUE_DRIVER_LOW: usize = 0x090;
    pub const REG_QUEUE_DRIVER_HIGH: usize = 0x094;
    pub const REG_QUEUE_DEVICE_LOW: usize = 0x0a0;
    pub const REG_QUEUE_DEVICE_HIGH: usize = 0x0a4;
    /// legacy 版本的队列设置: 一次告知一个页框号 (PFN), 三段区域须位于
    /// 连续的页里, 由设备按固定偏移自己算出位置。
    pub const REG_QUEUE_PFN: usize = 0x040;
    /// legacy 的 guest page size 寄存器。
    pub const REG_GUEST_PAGE_SIZE: usize = 0x028;
} // mod regs

use regs::*;

/// 设备状态位 (virtio 规范第 2.1 节), 必须按顺序置位:
/// `ACKNOWLEDGE -> DRIVER -> (读特性) -> FEATURES_OK -> (读回确认) -> DRIVER_OK`。
/// 跳步或顺序错, 设备会拒绝服务且不给错误提示 (后续 I/O 永远超时)。
mod status {
    pub const ACKNOWLEDGE: u32 = 1;
    pub const DRIVER: u32 = 2;
    pub const DRIVER_OK: u32 = 4;
    pub const FEATURES_OK: u32 = 8;
}

/// VirtIO 块设备驱动。
pub struct VirtioBlk {
    /// 寄存器窗口。
    regs: Mmio,
    /// 设备的 MMIO 槽位基地址。
    base: usize,
    /// 协议版本 (1 = legacy, 2 = modern)。
    version: u32,
    /// 设备容量, 单位扇区。
    capacity: u64,
    /// 队列大小 (描述符个数)。
    queue_size: u16,
    /// 协商后的特性位 (低 32 位)。
    features_lo: u32,
    /// 协商后的特性位 (高 32 位)。
    features_hi: u32,
    /// virtqueue 是否已经建立。
    ///
    /// 区分 `probe()` (只做握手) 与 `setup_queue()` (建立队列): 后者未调
    /// 就 read/write, 队列内存全零, 行为未定义 —— 用标志把这种情况变成
    /// 明确的 `NotReady` 错误。
    ready: bool,
    /// 已经处理完的 used ring 游标 (驱动视角)。设备每完成一个请求就把
    /// `used.idx` 加一, 驱动靠比较这两个值判断有没有新的完成事件。
    used_idx: u16,
    /// 下一个要使用的 avail slot。
    avail_idx: u16,
    /// virtqueue 内存 (由 `setup_queue` 登记)。
    ///
    /// 用裸指针而非 `&'static mut`: 后者在 `read`/`write` 里过不了借用
    /// 检查 (需从 self 取引用又可变借用 self)。裸指针是"诚实"的表达 ——
    /// 真实所有者是内核静态变量, 驱动只借地址, 由 `ready` 标志守住约束。
    queue_mem: Option<*mut VirtqueueMem>,
}

/// virtqueue 的三段内存 (desc/avail/used)。
///
/// 放进一个结构体并保持物理连续: legacy (v1) 协议只接受一个 PFN, 设备
/// 按固定布局推算三段位置。`used` ring 还必须是页对齐的, 用一个显式
/// 填充数组把它推到下一页, 而非依赖编译器恰好对齐。
#[repr(C, align(4096))]
pub struct VirtqueueMem {
    /// 第 0 页: 描述符表。
    pub desc: [VringDesc; MAX_QUEUE],
    /// avail ring (紧随描述符表之后)。
    pub avail: VringAvail,
    /// 填充到页边界 —— 让 `used` 正好落在下一页开头。
    _pad: [u8; 4096 - (MAX_QUEUE * 16 + 22)],
    /// 第 1 页: used ring (设备写, 驱动读)。
    pub used: VringUsed,
}

impl VirtqueueMem {
    /// 全零的队列内存 (常量表达式, 供 `static` 初始化)。
    ///
    /// 全零不能省: 设备可能在 `setup_queue` 返回后立刻读描述符, 残留的
    /// 随机数据会被当成合法地址去访问。
    pub const ZERO: Self = Self {
        desc: [VringDesc {
            addr: 0,
            len: 0,
            flags: 0,
            next: 0,
        }; MAX_QUEUE],
        avail: VringAvail {
            flags: 0,
            idx: 0,
            ring: [0; MAX_QUEUE],
            _pad: [0; 2],
        },
        _pad: [0; 4096 - (MAX_QUEUE * 16 + 22)],
        used: VringUsed {
            flags: 0,
            idx: 0,
            ring: [VringUsedElem { id: 0, len: 0 }; MAX_QUEUE],
        },
    };
}

/// 一个描述符 (16 字节)。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VringDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// avail ring: 驱动告诉设备"有哪些描述符链可以处理"。
#[repr(C)]
pub struct VringAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; MAX_QUEUE],
    _pad: [u8; 2],
}

/// used ring 里的一个元素。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VringUsedElem {
    pub id: u32,
    pub len: u32,
}

/// used ring: 设备告诉驱动"哪些描述符链已经处理完"。
#[repr(C)]
pub struct VringUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VringUsedElem; MAX_QUEUE],
}

/// 描述符链的最大长度。8 足够 (每个请求只用 3 个描述符)。
pub const MAX_QUEUE: usize = 8;

/// 描述符标志。
mod desc_flags {
    /// 后面还有描述符 (构成链)。
    pub const NEXT: u16 = 1;
    /// 这块内存是"设备写、驱动读" (即数据从设备流向驱动)。
    pub const WRITE: u16 = 2;
}

// ---------------------------------------------------------------------------
// 编译期断言: virtqueue 的内存布局必须与设备约定的一致
// ---------------------------------------------------------------------------
// 与磁盘格式那组断言同理: 把这些"只存在于规范里"的约束变成编译器
// 强制的事实。布局错了不会编译失败, 而是运行期"请求永不完成"——
// 那种失败非常难定位。
const _: () = {
    assert!(core::mem::size_of::<VringDesc>() == 16, "描述符必须是 16 字节");
    assert!(core::mem::size_of::<VringUsedElem>() == 8, "used 元素必须是 8 字节");
    // used ring 必须落在结构体内的页边界上 (偏移 4096)。
    assert!(
        core::mem::offset_of!(VirtqueueMem, used) == 4096,
        "used ring 必须位于页边界 (legacy 协议要求)"
    );
};

/// 块设备请求头 (16 字节)。
///
/// 布局由 VirtIO 规范规定, 不能随意改动。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BlkReq {
    pub type_: u32,
    pub reserved: u32,
    pub sector: u64,
}

/// 请求类型。
pub const BLK_T_IN: u32 = 0; // 读 (设备 -> 驱动)
pub const BLK_T_OUT: u32 = 1; // 写 (驱动 -> 设备)

impl VirtioBlk {
    /// 探测一个 virtio-mmio 槽位。
    ///
    /// 返回 `Some(driver)` 表示该槽位有 virtio 块设备且完成了状态机握手
    /// (但**尚未**建立 virtqueue, 那一步在 [`Self::setup_queue`])。
    ///
    /// 槽位可能为空或别种设备, 必须逐一检查再当成块设备, 把错误定位在
    /// "设备识别"阶段, 而不是让文件系统解析坏数据时崩溃。
    ///
    /// # Safety
    /// `base` 必须指向一个已映射的 virtio-mmio 槽位。
    pub unsafe fn probe(base: usize) -> Option<Self> {
        // SAFETY: 由调用者保证 base 已映射。
        let regs = unsafe { Mmio::new(base) };

        // 1. 魔数。这是"这个地址上真的有 virtio 设备"的唯一可靠证据。
        // 空槽位在 QEMU 上读回 0, 在真机上可能是总线错误。
        // SAFETY: 偏移 0 在 4 KiB 槽位内。
        let magic = unsafe { regs.read_u32(REG_MAGIC) };
        if magic != MAGIC {
            return None;
        }

        // 2. 版本。
        // SAFETY: 偏移 4。
        let version = unsafe { regs.read_u32(REG_VERSION) };
        if version != VERSION_LEGACY && version != VERSION_MODERN {
            return None;
        }

        // 3. 设备类型。只接受块设备。
        // SAFETY: 偏移 8。
        let device_id = unsafe { regs.read_u32(REG_DEVICE_ID) };
        if device_id != DEVICE_ID_BLOCK {
            return None;
        }

        let mut dev = Self {
            regs,
            base,
            version,
            capacity: 0,
            queue_size: 0,
            features_lo: 0,
            features_hi: 0,
            ready: false,
            used_idx: 0,
            avail_idx: 0,
            queue_mem: None,
        };

        // 4. 状态机握手。
        //
        // 每一步都不能省: ACKNOWLEDGE/DRIVER 表示"看到你/知道怎么驱动"。
        // 读特性发生在 FEATURES_OK 之前。FEATURES_OK 后必须**读回确认**:
        // 若设备拒绝特性组合, 该位不置上, 必须停止, 继续则行为未定义。
        // DRIVER_OK 表示"开始工作"。
        if dev.handshake().is_err() {
            return None;
        }

        Some(dev)
    }

    /// 执行 virtio 状态机握手并读取容量。
    fn handshake(&mut self) -> Result<(), BlockError> {
        // SAFETY: 下面所有访问都在 4 KiB 的 virtio-mmio 槽位内。
        unsafe {
            // --- 状态: 复位 -> ACKNOWLEDGE -> DRIVER ---
            self.regs.write_u32(REG_STATUS, 0);
            self.regs.write_u32(REG_STATUS, status::ACKNOWLEDGE);
            self.regs
                .write_u32(REG_STATUS, status::ACKNOWLEDGE | status::DRIVER);

            // --- 读设备特性 (只用低 32 位) ---
            self.regs.write_u32(REG_DEVICE_FEATURES_SEL, 0);
            self.features_lo = self.regs.read_u32(REG_DEVICE_FEATURES);
            self.regs.write_u32(REG_DEVICE_FEATURES_SEL, 1);
            self.features_hi = self.regs.read_u32(REG_DEVICE_FEATURES);

            // --- 提出驱动特性 ---
            //
            // 特性协商: 只接受必须的, 其余全拒绝。VIRTIO_F_VERSION_1 (bit 32)
            // **仅 modern (v2)** 可置; legacy 设备置上却用 legacy 的 PFN 设
            // 队列, 两边对队列理解不一致, 请求永远不完成。其余块设备特性
            // (RO/FLUSH/MQ) 一律拒绝 —— 协商了却不实现会让设备做未预期的
            // 行为。
            let want_lo: u32 = 0;
            let want_hi: u32 = if self.version == VERSION_MODERN {
                1 // VIRTIO_F_VERSION_1: 仅 modern 设备需要且允许
            } else {
                0 // legacy 设备: 不接受任何高 32 位特性
            };
            self.regs.write_u32(REG_DRIVER_FEATURES_SEL, 0);
            self.regs.write_u32(REG_DRIVER_FEATURES, want_lo);
            self.regs.write_u32(REG_DRIVER_FEATURES_SEL, 1);
            self.regs.write_u32(REG_DRIVER_FEATURES, want_hi);

            // --- FEATURES_OK, 然后**读回确认** ---
            self.regs.write_u32(
                REG_STATUS,
                status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK,
            );
            let st = self.regs.read_u32(REG_STATUS);
            if st & status::FEATURES_OK == 0 {
                // 协商失败。设备拒绝了我们提出的特性组合。
                // 这时**不能**继续 —— 规范明确要求此时停止初始化。
                return Err(BlockError::DeviceError);
            }

            // --- 队列信息 ---
            self.regs.write_u32(REG_QUEUE_SEL, 0);
            let qmax = self.regs.read_u32(REG_QUEUE_NUM_MAX);
            if qmax == 0 {
                // 队列 0 不存在 -> 这不是一个可用的块设备。
                return Err(BlockError::NotReady);
            }
            // ---- 队列长度: 取"设备允许的"与"我们预留的"之较小者 ----
            // 通过 QUEUE_NUM 告诉设备我们只用前 N 个。驱动有权选择比
            // 设备最大值更小的队列 (吞吐低一些, 功能完全一样), 反过来
            // 则不允许 —— 直接用 128 会让驱动预留的 8 个槽位被写越界。
            let want = core::cmp::min(qmax as usize, MAX_QUEUE);
            self.regs.write_u32(REG_QUEUE_NUM, want as u32);
            self.queue_size = want as u16;

            // --- DRIVER_OK ---
            self.regs.write_u32(
                REG_STATUS,
                status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
            );
        }

        // --- 读容量 ---
        //
        // 容量在 virtio-blk 的配置空间里 (槽位偏移 0x100 起), 一个 64 位
        // 扇区数, 分两个 32 位寄存器 (0x100 低, 0x104 高)。必须按"低再高"
        // 的顺序读: 某些 QEMU 版本的配置空间读是带副作用的 (读高会锁存低)。
        // SAFETY: 0x100/0x104 在槽位内。
        unsafe {
            let lo = self.regs.read_u32(0x100) as u64;
            let hi = self.regs.read_u32(0x104) as u64;
            self.capacity = lo | (hi << 32);
        }

        if self.capacity == 0 {
            // 容量为 0 的设备无法使用 (也没有意义)。
            return Err(BlockError::NotReady);
        }
        Ok(())
    }

    /// 基地址。
    pub fn base(&self) -> usize {
        self.base
    }

    /// 协议版本 (1 = legacy, 2 = modern)。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 协商后的队列大小。
    pub fn queue_size(&self) -> u16 {
        self.queue_size
    }

    /// 设备支持的特性位 (低 32 位)。
    pub fn device_features_lo(&self) -> u32 {
        self.features_lo
    }

    /// 设备支持的特性位 (高 32 位)。
    pub fn device_features_hi(&self) -> u32 {
        self.features_hi
    }

    /// 该设备是否支持"只读"特性。
    pub fn is_read_only(&self) -> bool {
        // VIRTIO_BLK_F_RO 是 bit 5。
        self.features_lo & (1 << 5) != 0
    }

    /// 扫描一段 virtio-mmio 槽位区域, 返回第一个块设备。
    ///
    /// 槽位分配取决于 QEMU 命令行设备顺序, 硬编码槽位 0 会一改就失效。
    /// 槽位步长固定 0x1000 (每槽位一页, 便于独立映射到不同 guest 页)。
    ///
    /// # Safety
    /// `[base, base + count * 0x1000)` 必须已经被映射。
    pub unsafe fn scan(plat: &Platform) -> Option<Self> {
        if plat.virtio_count == 0 || plat.virtio0_base == 0 {
            // 该平台没有 virtio-mmio 槽位。
            return None;
        }
        for i in 0..plat.virtio_count {
            let base = plat.virtio0_base + i * 0x1000;
            // SAFETY: 由调用者保证整段槽位已映射。
            if let Some(dev) = unsafe { Self::probe(base) } {
                return Some(dev);
            }
        }
        None
    }
}

impl VirtioBlk {
    /// 建立 virtqueue。
    ///
    /// * `mem` 提供 virtqueue 三段内存, 必须物理连续且已清零 (清零不能省:
    ///   设备可能在返回后立刻读描述符, 残留的随机数据会被当合法地址)。
    ///
    /// 两种协议不同: modern 分别告知 desc/avail/used 的物理地址;
    /// legacy 只告知一个 PFN, 设备自己按固定偏移推算。QEMU 的 virtio-mmio
    /// 默认是 legacy, 只实现 modern 会在 QEMU 上"队列永远不就绪"。
    ///
    /// # Safety
    /// `mem` 必须指向大小至少 `size_of::<VirtqueueMem>()`、物理连续、
    /// 且在整个驱动生命周期内不被移动的内存。
    pub unsafe fn setup_queue(&mut self, mem: *mut VirtqueueMem) -> Result<(), BlockError> {
        // 到这一步 queue_size 已是协商后的值, 一定 <= MAX_QUEUE。再查一次
        // 防止 handshake 改动后不变量被破坏 —— 越界访问描述符表是内存破坏。
        if self.queue_size == 0 || self.queue_size as usize > MAX_QUEUE {
            return Err(BlockError::NotReady);
        }

        let base = mem as usize;

        // SAFETY: 下面所有 MMIO 访问都在槽位内; mem 由调用者保证有效。
        unsafe {
            // 设备要读描述符, 先把队列内存清零, 否则残留被当合法描述符。
            core::ptr::write_bytes(mem, 0, 1);

            // ---- 重新选中队列 0, 并重设 QUEUE_NUM ----
            //
            // `QUEUE_SEL` 是设备侧"当前操作队列"游标, 规范不保证在两次
            // 访问间被保持, 不重选会把 PFN 写到别的队列。legacy 的队列
            // 布局 (三段位置) 在写 PFN 那一刻由 QUEUE_NUM 决定, 那时不对
            // 设备算出的 avail/used 偏移就与我们不一致。
            self.regs.write_u32(REG_QUEUE_SEL, 0);
            self.regs.write_u32(REG_QUEUE_NUM, self.queue_size as u32);

            if self.version == VERSION_LEGACY {
                // ---- legacy: 设置 guest 页大小, 然后告知 PFN ----
                // 旧版规范假设客户机页大小可能不是 4 KiB, 不设置会让设备
                // 按自己的默认值推算布局, 与我们错位。
                self.regs.write_u32(REG_GUEST_PAGE_SIZE, 4096);
                // PFN = 物理地址 >> 12 (设备要的是页框号)。
                self.regs.write_u32(REG_QUEUE_PFN, (base >> 12) as u32);
                // 旧版没有 QUEUE_READY 寄存器 —— 写 QUEUE_PFN 本身就是"已就绪"。
            } else {
                // ---- modern: 分别告知三个区域的地址 ----
                let desc = base + core::mem::offset_of!(VirtqueueMem, desc);
                let avail = base + core::mem::offset_of!(VirtqueueMem, avail);
                let used = base + core::mem::offset_of!(VirtqueueMem, used);
                self.regs.write_u32(REG_QUEUE_DESC_LOW, desc as u32);
                self.regs.write_u32(REG_QUEUE_DESC_HIGH, (desc >> 32) as u32);
                self.regs.write_u32(REG_QUEUE_DRIVER_LOW, avail as u32);
                self.regs.write_u32(REG_QUEUE_DRIVER_HIGH, (avail >> 32) as u32);
                self.regs.write_u32(REG_QUEUE_DEVICE_LOW, used as u32);
                self.regs.write_u32(REG_QUEUE_DEVICE_HIGH, (used >> 32) as u32);
                self.regs.write_u32(REG_QUEUE_READY, 1);
            }
        }

        self.ready = true;
        self.used_idx = 0;
        self.avail_idx = 0;
        self.queue_mem = Some(mem);
        Ok(())
    }

    /// 用队列发一个请求, 等它完成。
    ///
    /// 每个请求由 3 个描述符组成: 0=请求头 (设备读), 1=数据缓冲 (读操作
    /// 设备写 / 写操作设备读), 2=状态字节 (设备写)。
    ///
    /// 提交顺序不能乱: 填描述符 -> 写 avail.ring[] -> I/O 屏障 -> avail.idx++
    /// -> I/O 屏障 -> 写 QUEUE_NOTIFY。`avail.idx` 递增是"发布新请求"的信号,
    /// 若它先于描述符内容对设备可见, 设备会读到未写好的描述符。
    ///
    /// # Safety
    /// `mem` 必须与 `setup_queue` 传入的是同一块内存。
    unsafe fn submit(
        &mut self,
        mem: *mut VirtqueueMem,
        sector: u64,
        buf: &mut [u8],
        is_write: bool,
    ) -> Result<(), BlockError> { Err(BlockError::Unsupported) }
}

// SAFETY: `VirtioBlk` 里唯一的裸指针是 `queue_mem`, 它指向内核拥有的静态
// 队列内存。所有访问都通过 `BlockDevice` 的方法, 调用侧用锁保证同一时刻
// 只有一个 hart 在用 —— 跨 hart 传递的是"独占使用权", 不是并发访问。
unsafe impl Send for VirtioBlk {}

impl BlockDevice for VirtioBlk {
    fn name(&self) -> &'static str {
        "virtio-blk"
    }

    fn capacity_sectors(&self) -> u64 {
        self.capacity
    }

    fn read(&mut self, block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        // 先做参数检查 —— 无论哪个平台, 越界访问必须拦住, 而不是把越界
        // 请求发给设备 (那可能读到别的设备的数据)。
        let _count = super::check_request(self.capacity, block, buf.len())?;

        // 一次请求只读一个扇区 (512 字节), 超过就循环多次。virtio-blk 支持
        // 一次多扇区, 但调用者 (缓冲区缓存) 一次只读一块, 循环即可, 少一条
        // 需要测试的路径。
        let Some(mem) = self.queue_mem else {
            return Err(BlockError::NotReady);
        };
        let mut off = 0;
        while off < buf.len() {
            let n = core::cmp::min(512, buf.len() - off);
            // SAFETY: queue_mem 是 setup_queue 时登记的那块内存,
            // 由内核拥有且不会被移动。
            unsafe {
                self.submit(mem, block + (off / 512) as u64, &mut buf[off..off + n], false)?;
            }
            off += n;
        }
        Ok(())
    }

    fn write(&mut self, block: u64, buf: &[u8]) -> Result<(), BlockError> {
        let _count = super::check_request(self.capacity, block, buf.len())?;
        if self.is_read_only() {
            return Err(BlockError::DeviceError);
        }

        let Some(mem) = self.queue_mem else {
            return Err(BlockError::NotReady);
        };
        let mut off = 0;
        while off < buf.len() {
            let n = core::cmp::min(512, buf.len() - off);
            // submit 接受 `&mut [u8]` (读操作要写它); 写路径只需读这块内存。
            // 从不可变切片取地址重新构造可变引用 —— 因为 submit 对写路径只读,
            // 不违反别名规则。不用 transmute, 是避免掩盖"这块内存其实是只读的"。
            let ptr = buf[off..off + n].as_ptr() as *mut u8;
            // SAFETY: 写路径下 submit 不会写这块缓冲区 (只把地址告诉设备,
            // 要求设备读); ptr 来自有效切片, 长度 n。
            let slice = unsafe { core::slice::from_raw_parts_mut(ptr, n) };
            unsafe {
                self.submit(mem, block + (off / 512) as u64, slice, true)?;
            }
            off += n;
        }
        Ok(())
    }
}

// ===========================================================================
// 编译期自检
// ===========================================================================
const _: () = {
    // virtio-mmio 的配置空间在偏移 0x100 起 —— 它必须落在
    // 4 KiB 的槽位里, 否则不同的槽位会互相覆盖。
    assert!(REG_QUEUE_DEVICE_HIGH < 0x100);
    assert!(0x100 + 8 <= 0x1000);
    // 魔数就是 ASCII "virt" 的小端表示。
    assert!(MAGIC == 0x7472_6976);
    // 状态位必须是 2 的幂 (它们是独立的标志位)。
    assert!(status::ACKNOWLEDGE.is_power_of_two());
    assert!(status::DRIVER.is_power_of_two());
    assert!(status::DRIVER_OK.is_power_of_two());
    assert!(status::FEATURES_OK.is_power_of_two());
};
