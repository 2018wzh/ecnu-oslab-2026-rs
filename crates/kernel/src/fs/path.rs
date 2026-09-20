use super::inode::InodeRef;
// TODO(lab-9): 按 inode 锁保护目录修改；新目录建立 . 和 ..；失败回滚。
pub fn create(_path: &[u8], _kind: u16, _major: u16, _minor: u16) -> Result<InodeRef, ()> { todo!("lab-9: path::create") }
// TODO(lab-9): 普通文件硬链接，先增 nlink，再建目录项；失败回滚；拒绝目录硬链接。
pub fn link(_old: &[u8], _new: &[u8]) -> Result<(), ()> { todo!("lab-9: path::link") }
// TODO(lab-9): 删除目录项并减 nlink，InodeRef::Drop 在最后引用时判断资源回收。
pub fn unlink(_path: &[u8]) -> Result<(), ()> { todo!("lab-9: path::unlink") }
// TODO(lab-9): 经 .. 回溯，用 dentry::search_number 在父目录反查名字，根为 /；从后往前填充，返回起始偏移，dst[offset..] 是路径（含 NUL），容量不足 Err。
pub fn inode_to_path(_inode: &InodeRef, _dst: &mut [u8]) -> Result<usize, ()> { todo!("lab-9: path::inode_to_path") }
