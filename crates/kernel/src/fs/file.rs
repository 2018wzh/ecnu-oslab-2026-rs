use super::inode::{InodeRef, ReadDst, WriteSrc};
use crate::lock::SpinLock;
pub const N_FILE: usize = 128;
pub const N_FD: usize = 10;
pub struct File { inode: Option<InodeRef>, refs: usize, offset: u32, readable: bool, writable: bool }
impl File { pub const EMPTY: Self = Self { inode: None, refs: 0, offset: 0, readable: false, writable: false }; }
pub static TABLE_LOCK: SpinLock = SpinLock::UNINIT;
pub static mut TABLE: [File; N_FILE] = [const { File::EMPTY }; N_FILE];
pub struct FileRef { file: *mut File }
impl FileRef {
    // TODO(lab-9): 表锁内增引用，共享同一个 offset。
    pub fn dup(&self) -> Self { todo!("lab-9: FileRef::dup") }
    // TODO(lab-9): 普通文件 inode 锁串行化共享偏移读改写，按 inode/设备类型读取 ReadDst（用户地址不能转引用）。
    pub fn read(&self, _dst: ReadDst<'_>) -> Result<usize, ()> { todo!("lab-9: FileRef::read") }
    // TODO(lab-9): 权限/类型检查，拒绝写目录，持 inode 锁更新实际完成量及共享偏移。
    pub fn write(&self, _src: WriteSrc<'_>) -> Result<usize, ()> { todo!("lab-9: FileRef::write") }
    // TODO(lab-9): unsigned offset，SET/ADD/SUB 尽力而为；inode 锁保护共享偏移。
    pub fn seek(&self, _offset: u32, _whence: usize) -> Result<u32, ()> { todo!("lab-9: FileRef::seek") }
    // TODO(lab-9): inode 锁下形成固定布局 stat，无未初始化 padding。
    pub fn stat(&self) -> Result<oslab_uapi::FileStat, ()> { todo!("lab-9: FileRef::stat") }
}
impl Drop for FileRef {
    // TODO(lab-9): 减引用，最后引用释放 inode；不能持表自旋锁做 I/O。
    fn drop(&mut self) { todo!("lab-9: FileRef::drop") }
}
// TODO(lab-9): 初始化表和锁。
pub fn init() { todo!("lab-9: file::init") }
// TODO(lab-9): refs=0 槽位分配，完成初值后返回。
pub fn alloc() -> Result<FileRef, ()> { todo!("lab-9: file::alloc") }
// TODO(lab-9): 解析路径、OPEN_CREATE、OPEN_READ/OPEN_WRITE 与类型检查。
pub fn open(_path: &[u8], _mode: usize) -> Result<FileRef, ()> { todo!("lab-9: file::open") }
