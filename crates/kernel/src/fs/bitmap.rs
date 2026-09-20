/// 教师诊断：传入 fs::init 校验完成的超级块，每次只持有一个位图守卫。
pub fn print(sb: &super::Superblock, print_data: bool) {
    let inode = !print_data;
    let (first, blocks, total, base) = if inode {
        (sb.inode_bitmap, sb.inode_bitmap_blocks, sb.total_inodes, 0)
    } else {
        (sb.data_bitmap, sb.data_bitmap_blocks, sb.data_blocks, sb.data_first)
    };
    let bits = super::BLOCK_SIZE as u64 * 8;
    if (blocks as u64) * bits < total as u64
        || first as u64 + blocks as u64 > sb.total_blocks as u64
        || (!inode && base as u64 + total as u64 > sb.total_blocks as u64) {
        panic!("bitmap layout");
    }
    crate::println!("{} bitmap alloced bits:", if inode { "inode" } else { "data" });
    let mut start = 0u64;
    while start < total as u64 {
        let buffer = super::buffer::get(first + (start / bits) as u32).expect("bitmap read");
        let valid = (total as u64 - start).min(bits) as usize;
        for bit in 0..valid {
            if buffer.data()[bit / 8] & (1 << (bit % 8)) != 0 {
                crate::print!("{} ", base as u64 + start + bit as u64);
            }
        }
        start += bits;
        // 本轮结束释放 buffer；I/O 失败也不会遗留先前的守卫。
    }
    crate::println!("over!\n");
}
// TODO(lab-7): 获取位图缓存，置位后写盘并归还；单个位图块内扫描 valid 个有效 bit，置首个零位，返回块内 bit 号；满返回 None。
pub fn search_and_set(_bitmap_block: u32, _valid: usize) -> Option<u32> { todo!("lab-7: bitmap::search_and_set") }
// TODO(lab-7): 将单个位图块中 bit 清零。获取位图缓存、清位、写盘并归还。
pub fn clear(_bitmap_block: u32, _bit: usize) { todo!("lab-7: bitmap::clear") }
// TODO(lab-7): 跨位图块扫描、置位并写回，返回绝对块号；耗尽 panic。
pub fn alloc_block() -> u32 { todo!("lab-7: bitmap::alloc_block") }
// TODO(lab-7): 跨位图块扫描、置位并写回，返回 inode 号（包括 0）；耗尽 panic。
pub fn alloc_inode() -> u32 { todo!("lab-7: bitmap::alloc_inode") }
// TODO(lab-7): 将绝对块号转为位图位置，清位并写回。
pub fn free_block(_number: u32) { todo!("lab-7: bitmap::free_block") }
// TODO(lab-7): 将 inode 号转为位图位置，清位并写回。
pub fn free_inode(_number: u32) { todo!("lab-7: bitmap::free_inode") }
