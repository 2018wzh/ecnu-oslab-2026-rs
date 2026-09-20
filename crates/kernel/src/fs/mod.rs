pub mod block;
pub mod tokens;
pub mod buffer;
pub mod bitmap;
pub use oslab_uapi::disk::*;
pub struct Superblock { pub magic: u32, pub block_size: u32, pub total_blocks: u32, pub total_inodes: u32,
    pub inode_bitmap: u32, pub inode_bitmap_blocks: u32, pub inode_first: u32, pub inode_blocks: u32,
    pub data_bitmap: u32, pub data_bitmap_blocks: u32, pub data_first: u32, pub data_blocks: u32 }
pub static mut SUPERBLOCK: Option<Superblock> = None;
impl Superblock {
    /// 输出超级块与磁盘布局信息（教师诊断）；对象已经过 init 校验。
    pub fn print(&self) {
        crate::println!("superblock: magic={:#x} block_size={}", self.magic, self.block_size);
        crate::println!("total_blocks={} total_inodes={}", self.total_blocks, self.total_inodes);
        crate::println!("inode_bitmap={} blocks={} inode_first={} blocks={}",
            self.inode_bitmap, self.inode_bitmap_blocks, self.inode_first, self.inode_blocks);
        crate::println!("data_bitmap={} blocks={} data_first={} blocks={}",
            self.data_bitmap, self.data_bitmap_blocks, self.data_first, self.data_blocks);
    }
}
// TODO(lab-7): tokens::init 单次初始化教师令牌外围。
// TODO(lab-7): 首进程上下文初始化 buffer，读块 0，LE 解码并校验布局，打印信息。
pub fn init() { todo!("lab-7: fs::init") }
