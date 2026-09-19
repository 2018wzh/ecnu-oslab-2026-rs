//! 位图分配器: 用位图记录哪些数据块 / inode 空闲。第 i 位为 1 表示
//! 第 i 个资源已占用。一个 4 KiB 块有 32768 位, 即能描述 32768 个资源,
//! 比空闲链表的"每个空闲块存一个指针"更适合小块、细粒度的分配。
//!
//! 位序与字节序是两件事: 位号 i 落在字节 i/8、位 i%8。本内核约定
//! 最低位优先 (与 ext2 一致), 由编译期自检验证。

// 位图不持有缓冲区: 反复读写都经过缓冲区缓存, 持有一个引用会让
// "什么时候释放"变复杂。这里只保存位图在磁盘上的位置。
/// 一个位图。
#[derive(Debug, Clone, Copy)]
pub struct Bitmap {
    /// 位图起始块号。
    pub start_block: usize,
    /// 位图占多少个块。
    pub blocks: usize,
    /// 一共管理多少个资源 (位)。
    pub total: usize,
}

impl Bitmap {
    // 需要显式记录 `total`: 最后一个块的尾部可能有空洞, 位图长度
    // 通常不是 `blocks * BLOCK_SIZE * 8` 的整数倍, 否则那些空洞会被
    // 当成可分配的资源, 分配出去后访问到不存在的块或 inode。
    /// 描述一个位图。
    pub const fn new(start_block: usize, blocks: usize, total: usize) -> Self {
        Self {
            start_block,
            blocks,
            total,
        }
    }

    /// 第 `i` 位落在哪个字节里 (相对于位图起点)。
    pub const fn byte_index(i: usize) -> usize {
        i / 8
    }

    // 位 i%8 取最低位还是最高位由磁盘格式规定 (ext2 用最低位)。
    /// 第 `i` 位是那个字节的第几位。
    pub const fn bit_index(i: usize) -> usize {
        i % 8
    }

    /// 位图需要的字节数。
    pub const fn bytes_needed(&self) -> usize {
        (self.total + 7) / 8
    }

    /// 位图需要的块数 (向上取整)。
    pub const fn blocks_needed(total: usize) -> usize {
        (total + 8 * 512 - 1) / (8 * 512)
    }
}

// 这个函数会解释磁盘上读来的数据, 数据可能是损坏的 —— 用越界的
// 下标读它会让内核访问缓冲区之外的内存。
/// 从一段位图字节里读第 `i` 位。
pub fn bit_get(map: &[u8], i: usize) -> bool {
    let byte = Bitmap::byte_index(i);
    if byte >= map.len() {
        return true; // 越界视为"已占用" —— 保守的选择
    }
    map[byte] & (1 << Bitmap::bit_index(i)) != 0
}

/// 设置位图中的第 `i` 位。
pub fn bit_set(map: &mut [u8], i: usize) {
    let byte = Bitmap::byte_index(i);
    if byte < map.len() {
        map[byte] |= 1 << Bitmap::bit_index(i);
    }
}

/// 清掉位图中的第 `i` 位。
pub fn bit_clear(map: &mut [u8], i: usize) {
    let byte = Bitmap::byte_index(i);
    if byte < map.len() {
        map[byte] &= !(1 << Bitmap::bit_index(i));
    }
}

// 从 0 开始线性扫描, 简单且让分配偏向低编号, 相关的东西在磁盘上
// 靠近 (局部性); 代价是最坏要扫完整张位图, 教学内核里可预测性更有价值。
/// 在位图里找第一个为空 (0) 的位。返回 `None` 表示没有空闲资源。
pub fn find_free(map: &[u8], total: usize) -> Option<usize> { unimplemented!() }

// ---------------------------------------------------------------------------
// 自检: 位序必须与约定一致
// ---------------------------------------------------------------------------
// 位序约定写错不会编译失败、也不会立刻崩溃, 只在与真实磁盘格式交互时
// 暴露, 所以用编译期断言把它钉住。
const _: () = {
    // 位 0 必须是第一个字节的 bit 0 (最低位)。
    assert!(Bitmap::byte_index(0) == 0);
    assert!(Bitmap::bit_index(0) == 0);
    // 位 8 必须换到下一个字节。
    assert!(Bitmap::byte_index(8) == 1);
    assert!(Bitmap::bit_index(8) == 0);
    // 位 9 是第二个字节的 bit 1。
    assert!(Bitmap::byte_index(9) == 1);
    assert!(Bitmap::bit_index(9) == 1);
};