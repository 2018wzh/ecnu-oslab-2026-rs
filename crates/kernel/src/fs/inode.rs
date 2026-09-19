//! 索引节点: 一个文件的全部元数据, 以及它数据在磁盘的哪些块里。
//! 它不含文件名 —— 名字属于目录, 不属于文件 (目录: 名字 -> inode 号,
//! inode: inode 号 -> 元数据 + 数据块位置)。硬链接因此自然: 两个名字
//! 指向同一个 inode 号即可。
//!
//! 磁盘格式 (`DiskInode`, 紧凑/小端/位置固定) 与内存表示 (`Inode`,
//! 带锁/带引用计数) 必须经 `from_disk` / `to_disk` 显式转换, 不能把
//! 磁盘缓冲区直接当结构体读 (对齐、字节序、一致性三个理由)。

// 这个常量在 `bio`、`inode`、`dir` 各有一份: 各层"块大小"含义略不同,
// 三份都是 512 且有编译期断言与 ABI 规范对照, 不存在改其一忘其一的风险。
/// 块大小 (字节)。与块设备的扇区一致, 也是 ABI 规范规定的磁盘块大小。
pub const BLOCK_SIZE: usize = 512;

// 12 是 ext2 的传统值, 也是权衡: 太小则小文件就要走间接块 (每次读多一次
// 磁盘访问); 太大则 inode 变大, 而多数文件是小文件, inode 表浪费磁盘。
// 12 个直接块 = 6 KiB, 加上一级间接 (128 块 = 64 KiB) 覆盖绝大多数文件。
/// 直接块的数量。
pub const NDIRECT: usize = 12;

// 一块 512 字节 ÷ 一个块号 4 字节 = 128。手写 128 容易把"512 字节"
// 误当成"512 个块号", 让间接块计算偏大 4 倍; 用 `size_of` 算出来,
// 将来块大小或块号宽度变了会自动跟上。
/// 每个块能放多少个块号。
pub const NINDIRECT: usize = BLOCK_SIZE / core::mem::size_of::<u32>();

// 布局来自 `docs/abi-spec.md` 第 5 节: dtype(0)/major(2)/minor(4)/nlink(6)/
// size(8)/addrs(12, 4x13), 合计 64 字节。`#[repr(C, packed)]` 去掉填充,
// 让"字段偏移 = 宽度之和"成为硬约束, 否则将来加一个 `u8` 字段填充就会
// 出现而磁盘格式不能变; packed 代价是读对齐字段是非对齐访问, 必须逐字节
// 组装 (见 [`DiskInode::from_bytes`] / [`DiskInode::to_bytes`])。
/// 磁盘上的 inode 结构 (64 字节, **紧凑无填充**)。
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DiskInode {
    /// 文件类型: 0=空闲, 1=目录, 2=文件, 3=设备。
    pub dtype: u16,
    /// 设备主号 (普通文件为 0)。
    pub major: u16,
    /// 设备次号 (普通文件为 0)。
    pub minor: u16,
    /// 硬链接数。
    pub nlink: u16,
    /// 文件大小 (字节)。
    pub size: u32,
    // 与 ext2 (12 直接 + 二级/三级间接, 15 个地址) 不同, 本课程按
    // `docs/abi-spec.md` 用 13 个地址, 只有 12 直接 + 1 一级间接, 单文件
    // 上限 70 KB —— 教学取舍: 足够跑完所有测试, 而少一层间接少一处边界。
    /// 块地址: `addrs[0..12]` 是直接块, `addrs[12]` 是一级间接块。
    pub addrs: [u32; 13],
}

/// 磁盘 inode 的字节数 (ABI 规范规定: **正好 64**)。
pub const DISK_INODE_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// 编译期断言: 布局必须与 ABI 规范逐字节一致
// ---------------------------------------------------------------------------
// 只查总大小不够 —— 两个字段换位而总大小不变时断言仍通过。这里把每个
// 字段的偏移都钉住: C 版本改布局会让断言失败, 而不是等到镜像数据不一致。
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<DiskInode>() == DISK_INODE_SIZE);
    // 字段偏移逐项固定 (与 docs/abi-spec.md 第 5 节的表一致)。
    assert!(offset_of!(DiskInode, dtype) == 0);
    assert!(offset_of!(DiskInode, major) == 2);
    assert!(offset_of!(DiskInode, minor) == 4);
    assert!(offset_of!(DiskInode, nlink) == 6);
    assert!(offset_of!(DiskInode, size) == 8);
    assert!(offset_of!(DiskInode, addrs) == 12);
    // packed 结构体的对齐是 1 —— 这正是"不能直接按字段访问"的原因。
    assert!(align_of::<DiskInode>() == 1);
    // 13 个块地址共 52 字节 (13 * 4), 从偏移 12 开始 -> 结束于 64。
    assert!(size_of::<DiskInode>() - offset_of!(DiskInode, addrs) == 52);
    assert!(13 * 4 == 52);
    assert!(12 + 1 == 13);
};

// 逐字段组装而不是 `*(buf as *const DiskInode)`: packed 结构体里非对齐的
// `u32` 在 RISC-V 上解引用会触发异常; 磁盘是小端, 而"本机恰好也是小端"
// 不是可依赖的事实。
impl DiskInode {
    /// 从 64 字节的磁盘缓冲区解析出一个 inode。`buf` 必须至少 64 字节。
    pub fn parse(buf: &[u8]) -> Self { unimplemented!() }

    /// 一个空的 (全零) inode。
    pub const fn zeroed() -> Self {
        Self {
            dtype: 0,
            major: 0,
            minor: 0,
            nlink: 0,
            size: 0,
            addrs: [0; 13],
        }
    }

    // 逐字节组装而非直接指针转换: 对齐 (非对齐 `u32`/`u64` 在 RISC-V 上
    // 触发异常) 与字节序 (磁盘固定小端, 不能依赖"本机恰好是小端")。
    /// 从 64 字节的磁盘缓冲区解析出来。`buf` 不足 64 字节返回 `None`。
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < DISK_INODE_SIZE {
            return None;
        }
        let mut addrs = [0u32; 13];
        let mut i = 0;
        while i < 13 {
            addrs[i] = read_u32_le(buf, 12 + i * 4);
            i += 1;
        }
        Some(Self {
            dtype: read_u16_le(buf, 0),
            major: read_u16_le(buf, 2),
            minor: read_u16_le(buf, 4),
            nlink: read_u16_le(buf, 6),
            size: read_u32_le(buf, 8),
            addrs,
        })
    }

    /// 写进 64 字节的磁盘缓冲区。
    pub fn to_bytes(&self, buf: &mut [u8]) -> bool { unimplemented!() }

    /// 是否空闲。
    pub fn is_free(&self) -> bool {
        self.dtype == 0
    }
}

// ===========================================================================
// 小端读写原语
// ===========================================================================
// 自己写而不是用 `u32::from_le_bytes`: 后者要 `[u8; 4]`, 而这里要从
// 缓冲区任意偏移取字节; 也把"小端"显式写进名字里便于搜索。
/// 从 `buf[off..off+2]` 读一个小端 `u16`。
pub fn read_u16_le(buf: &[u8], off: usize) -> u16 {
    if off + 2 > buf.len() {
        return 0;
    }
    (buf[off] as u16) | ((buf[off + 1] as u16) << 8)
}

/// 从 `buf[off..off+4]` 读一个小端 `u32`。
pub fn read_u32_le(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        return 0;
    }
    (buf[off] as u32)
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24)
}

/// 往 `buf[off..off+2]` 写一个小端 `u16`。
pub fn write_u16_le(buf: &mut [u8], off: usize, v: u16) {
    if off + 2 > buf.len() {
        return;
    }
    buf[off] = (v & 0xff) as u8;
    buf[off + 1] = ((v >> 8) & 0xff) as u8;
}

/// 往 `buf[off..off+4]` 写一个小端 `u32`。
pub fn write_u32_le(buf: &mut [u8], off: usize, v: u32) {
    if off + 4 > buf.len() {
        return;
    }
    buf[off] = (v & 0xff) as u8;
    buf[off + 1] = ((v >> 8) & 0xff) as u8;
    buf[off + 2] = ((v >> 16) & 0xff) as u8;
    buf[off + 3] = ((v >> 24) & 0xff) as u8;
}

// 额外的字段都不在磁盘上, 它们描述"这一份内存副本"的状态; 与磁盘字段
// 混在一个结构体里会让"哪些字段要写回磁盘"变成需要记忆的问题。
/// 内存里的 inode。
#[derive(Debug, Clone, Copy)]
pub struct Inode {
    /// 磁盘上的那一份。
    pub disk: DiskInode,
    /// 内存引用计数。
    pub refs: usize,
    /// 内容是否有效。
    pub valid: bool,
    /// inode 号。
    pub inum: u32,
    /// 是否被修改过 (需要写回)。
    pub dirty: bool,
}

impl Inode {
    /// 一个无效的 inode。
    pub const fn empty() -> Self {
        Self {
            disk: DiskInode {
                dtype: 0,
                major: 0,
                minor: 0,
                nlink: 0,
                size: 0,
                addrs: [0; 13],
            },
            refs: 0,
            valid: false,
            inum: 0,
            dirty: false,
        }
    }
}

// 编号与直觉不同 (目录是 1, 文件是 2): 磁盘格式是契约, 数值由规范决定。
// 写反则镜像里目录被当普通文件打开, `open("/")` 成功但 read 出二进制垃圾。
/// 文件类型 (数值由 `docs/abi-spec.md` 第 5 节规定)。
pub mod file_type {
    /// 空闲 inode。
    pub const FREE: u16 = 0;
    /// 目录。
    pub const DIRECTORY: u16 = 1;
    /// 普通文件。
    pub const REGULAR: u16 = 2;
    /// 设备文件 (见 `fs::dev`)。
    pub const DEVICE: u16 = 3;
}

// TODO(lab-8): 实现 inode 的块映射 (含间接块)。
//   直接块 / 一级间接 / 二级间接的边界很容易差一, 建议
//   先用纸笔把块号区间写清楚再动手。
// ===========================================================================
// 块映射
// ===========================================================================
// 给定"文件内的第 n 个块", 算出"磁盘上的块号": 直接块 (n<12) 取
// addrs[n], 一级间接 (n<12+128) 读 addrs[12] 指向的块取其中第 n-12 项。
//
// 用固定大小的地址数组: inode 大小必须固定 (存在于定长记录组成的 inode
// 表, ABI 规定每条 64 字节), 动态数组会让"inode 号 * 64"无法直接算偏移。
// 只有 12 直接 + 1 间接: 单文件上限 70 KB 足够跑完实验, 少一处边界。

/// 文件的最大块数: 12 个直接 + 128 个一级间接 = 140。
pub const MAX_BLOCKS: usize = NDIRECT + NINDIRECT;

// 与 `docs/abi-spec.md` 第 5 节一致: `12*512 + 128*512 = 70 KB`。
/// 一个文件最大能有多大 (字节): 140 * 512 = 70 KB。
pub const MAX_FILE_SIZE: usize = MAX_BLOCKS * BLOCK_SIZE;

impl DiskInode {
    // "分层边界"是最容易差一的地方, 集中到一个函数里让边界只写一次、
    // 只测一次。返回 (层, 层内下标), 超出上限返回 `None`。
    /// 判断第 `n` 个块属于哪一层, 以及在该层里的下标。
    pub const fn block_level(n: usize) -> Option<(u8, usize)> {
        if n < NDIRECT {
            Some((0, n))
        } else if n < MAX_BLOCKS {
            Some((1, n - NDIRECT))
        } else {
            None // 超出文件大小上限
        }
    }
}

// ---------------------------------------------------------------------------
// 自检: 分层边界与 ABI 上限
// ---------------------------------------------------------------------------
// 边界差一的表现是"读文件时内容错位 512 字节", 看起来像别的 bug,
// 所以把边界钉死。
const _: () = {
    // 第 0 块是直接块。
    assert!(matches!(DiskInode::block_level(0), Some((0, 0))));
    // 第 11 块仍是直接块 (NDIRECT = 12, 下标 0..11)。
    assert!(matches!(DiskInode::block_level(11), Some((0, 11))));
    // 第 12 块开始是一级间接, 下标 0。
    assert!(matches!(DiskInode::block_level(12), Some((1, 0))));
    // 一级间接的最后一个 (NINDIRECT = 128, 下标 0..127)。
    assert!(matches!(DiskInode::block_level(12 + 127), Some((1, 127))));
    // 超出上限。
    assert!(DiskInode::block_level(MAX_BLOCKS).is_none());
    // ABI 规范里写明的文件上限: 70 KB。
    assert!(MAX_BLOCKS == 140);
    assert!(MAX_FILE_SIZE == 70 * 1024);
};