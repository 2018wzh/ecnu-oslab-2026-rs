use crate::lock::{SpinLock, sleeplock::{SleepLock, SleepGuard}};
pub const N_CACHE: usize = 64;
// 64 字节磁盘逻辑字段；kind/major/minor/nlink/size/index 按固定偏移 LE 编码。
pub struct DiskInode { pub kind: u16, pub major: u16, pub minor: u16, pub nlink: u16, pub size: u32, pub index: [u32; 13] }
impl DiskInode { pub const ZERO: Self = Self { kind: 0, major: 0, minor: 0, nlink: 0, size: 0, index: [0; 13] }; }
// info/valid 由睡眠锁保护；refs 由 CACHE_LOCK 保护；number 在活跃引用期间不变。
// N_CACHE=64 是内存缓存数量，磁盘 N_INODE=65536。
pub struct Inode { info: DiskInode, number: u32, refs: usize, valid: bool, lock: SleepLock }
impl Inode { pub const EMPTY: Self = Self { info: DiskInode::ZERO, number: 0, refs: 0, valid: false, lock: SleepLock::UNINIT }; }
pub static CACHE_LOCK: SpinLock = SpinLock::UNINIT;
pub static mut CACHE: [Inode; N_CACHE] = [const { Inode::EMPTY }; N_CACHE];
// 递归删除一个索引元素；level=0/1/2；返回是否遇到空块（文件末尾）。
// TODO(lab-8): 释放数据块及索引块，对应 __free_data_blocks。
fn free_block_tree(_block: u32, _level: usize) -> bool { todo!("lab-8: free_block_tree") }
/// 用户虚拟地址只保存整数；不得据此构造 Rust 引用。
pub struct UserAddr(pub usize);
pub enum ReadDst<'a> { Kernel(&'a mut [u8]), User { address: UserAddr, len: usize } }
pub enum WriteSrc<'a> { Kernel(&'a [u8]), User { address: UserAddr, len: usize } }
pub struct InodeRef { inode: *mut Inode }
pub struct InodeGuard<'a> { inode: &'a InodeRef, lock: SleepGuard<'a> }
impl InodeRef {
    pub fn number(&self) -> u32 {
        // SAFETY: 引用计数非零期间槽位身份不变。
        unsafe { (*self.inode).number }
    }
    // TODO(lab-8): 获取睡眠锁，valid=false 时从磁盘读取。
    pub fn lock(&self) -> InodeGuard<'_> { todo!("lab-8: InodeRef::lock") }
    // TODO(lab-8): cache 锁下增 refs。
    pub fn dup(&self) -> Self { todo!("lab-8: InodeRef::dup") }
}
impl Drop for InodeRef {
    // TODO(lab-8): 判断最后引用、有效信息及 nlink，满足删除条件时调用独立 delete；
    // 否则仅减引用。不持自旋锁做 I/O，不在此替代 delete 的学生实现。
    fn drop(&mut self) { todo!("lab-8: InodeRef::drop") }
}
impl InodeGuard<'_> {
    /// 教师外层遍历；连续文件遇空即停，递归仍由学生完成。
    pub fn free_blocks(&mut self) {
        for (i, &block) in self.info().index.iter().enumerate() {
            let level = if i < 10 { 0 } else if i < 12 { 1 } else { 2 };
            if free_block_tree(block, level) { return; }
        }
        panic!("free_data_blocks: impossible!");
    }
    /// 教师诊断，仅查看当前守卫保护的元数据。
    pub fn print(&self, name: &str) {
        let cache_guard = CACHE_LOCK.lock();
        // SAFETY: 活跃引用保证槽位有效；refs 读取受 cache 锁保护。
        let (refs, valid) = unsafe { ((*self.inode.inode).refs, (*self.inode.inode).valid) };
        crate::println!("inode {}: ref={} valid_info={}", name, refs, valid);
        drop(cache_guard);
        let info = self.info();
        crate::println!("inode={} type={} major={} minor={} nlink={} size={}",
            self.inode.number(), info.kind, info.major, info.minor, info.nlink, info.size);
        for (i, block) in info.index.iter().enumerate() { crate::println!("index[{}]={}", i, block); }
    }
    pub fn info(&self) -> &DiskInode {
        // SAFETY: 持有 inode 睡眠锁。
        unsafe { &(*self.inode.inode).info }
    }
    pub fn info_mut(&mut self) -> &mut DiskInode {
        // SAFETY: 守卫独占 info，引用不超过守卫生命周期。
        unsafe { &mut (*self.inode.inode).info }
    }
    /// 磁盘 inode 与内存 inode 的互相更新，持锁，按固定偏移 LE 编解码。
    // TODO(lab-8): write=false 读入；write=true 写回。
    pub fn rw(&mut self, _write: bool) { todo!("lab-8: InodeGuard::rw") }
    // TODO(lab-8): 已分配逻辑块或紧邻末尾的新块；10+2+1 映射，失败 Err。
    pub fn locate_or_add_block(&mut self, _logical: u32) -> Result<u32, ()> { todo!("lab-8: locate_or_add_block") }
    // TODO(lab-8): 独立删除任务，释放 inode 位及其管理的所有数据/索引块。
    pub fn delete(&mut self) { todo!("lab-8: InodeGuard::delete") }
    // TODO(lab-8): 以 buffer 为中介；Kernel 拷贝到切片，User 经页表检查与 copy_to_user。
    // 用户复制属于本任务，不能把 UserAddr 转成引用；返回实际字节数，EOF 短读/0。
    pub fn read_data(&mut self, _offset: u32, _dst: ReadDst<'_>) -> usize { todo!("lab-8: read_data") }
    // TODO(lab-8): Kernel 切片或 User 的 copy_from_user；检查长度/上限，更新 index/size 并写回。
    pub fn write_data(&mut self, _offset: u32, _src: WriteSrc<'_>) -> usize { todo!("lab-8: write_data") }
}
impl Drop for InodeGuard<'_> {
    // TODO(lab-8): 对应 inode_unlock，释放持锁生命周期；SleepGuard 负责底层解锁。
    fn drop(&mut self) { todo!("lab-8: inode_unlock") }
}
// TODO(lab-8): 初始化 cache、refs、睡眠锁。
pub fn init() { todo!("lab-8: inode::init") }
// TODO(lab-8): 查找/占用并增 refs，无空闲 cache 时 panic；返回未上锁引用。
pub fn get(_number: u32) -> InodeRef { todo!("lab-8: inode::get") }
// TODO(lab-8): 申请位图，初始化元数据后写盘，返回未上锁引用。
pub fn create(_kind: u16, _major: u16, _minor: u16) -> InodeRef { todo!("lab-8: inode::create") }
