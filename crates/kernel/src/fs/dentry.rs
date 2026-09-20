use super::{inode::{InodeRef, InodeGuard}, NAME_BYTES};
/// 教师路径拆分：跳过组件前后斜杠，名称最多复制 59 字节并补 NUL。
/// Rust 路径是无 NUL 的字节串；超长组件消费完整、名称截断。
pub fn element(path: &[u8]) -> Option<([u8; NAME_BYTES], &[u8])> {
    let begin = path.iter().position(|b| *b != b'/').unwrap_or(path.len());
    let path = &path[begin..];
    if path.is_empty() { return None; }
    let end = path.iter().position(|b| *b == b'/').unwrap_or(path.len());
    let mut name = [0; NAME_BYTES];
    let len = end.min(NAME_BYTES - 1);
    name[..len].copy_from_slice(&path[..len]);
    let mut rest = &path[end..];
    while rest.first() == Some(&b'/') { rest = &rest[1..]; }
    Some((name, rest))
}
/// 教师诊断：整块扫描有效目录项，不修改目录或链接数。
pub fn print(dir: &InodeGuard<'_>) -> Result<(), ()> {
    let info = dir.info();
    assert_eq!(info.kind, oslab_uapi::disk::INODE_DIR);
    assert_ne!(info.index[0], 0, "dentry_print: invalid index[0]");
    let block = super::buffer::get(info.index[0])?;
    for (i, entry) in block.data().chunks_exact(64).enumerate() {
        let number = u32::from_le_bytes(entry[60..64].try_into().map_err(|_| ())?);
        if entry[0] == 0 { continue; }
        let end = entry[..NAME_BYTES].iter().position(|b| *b == 0).ok_or(())?;
        // 名称是字节序列，调试输出不假定磁盘内容一定是 UTF-8。
        crate::println!("offset={} inode={} name={:?}", i * 64, number, &entry[..end]);
    }
    Ok(())
}
// TODO(lab-8): 单块目录内按名称查找，不存在 Err；空槽由 name[0]==0 判断；根 inode 0 合法。
pub fn search(_dir: &mut InodeGuard<'_>, _name: &[u8]) -> Result<u32, ()> { todo!("lab-8: dentry::search") }
// TODO(lab-8): 重名/目录满 Err；成功返回目录项字节偏移，size 记录有效项字节数。
pub fn create(_dir: &mut InodeGuard<'_>, _number: u32, _name: &[u8]) -> Result<u32, ()> { todo!("lab-8: dentry::create") }
// TODO(lab-8): 清槽并更新 size，成功返回被删除的 inode 编号，失败 Err。
pub fn delete(_dir: &mut InodeGuard<'_>, _name: &[u8]) -> Result<u32, ()> { todo!("lab-8: dentry::delete") }
// 对应 2025 __path_to_inode：两个公开入口共用一个学生实现。
// find_parent 为 true 时返回父目录引用与 NUL 填充的末级名称，根路径 Err；
// 否则返回目标引用，名称无意义。返回前释放锁守卫，InodeRef 转交调用者。
// TODO(lab-8): 绝对路径逐组件解析，检查类型/名称；逐级释放守卫/引用。
fn resolve(_path: &[u8], _find_parent: bool) -> Result<(InodeRef, [u8; NAME_BYTES]), ()> {
    todo!("lab-8: dentry::resolve")
}
/// 教师薄封装：仅选择查找模式，不实现路径遍历。
pub fn lookup(path: &[u8]) -> Result<InodeRef, ()> {
    resolve(path, false).map(|(inode, _)| inode)
}
pub fn parent(path: &[u8]) -> Result<(InodeRef, [u8; NAME_BYTES]), ()> {
    resolve(path, true)
}

// TODO(lab-9): 按 inode 编号反查名称并填入 name，返回名称字节长度（不含 NUL）。
pub fn search_number(_dir: &mut InodeGuard<'_>, _number: u32, _name: &mut [u8; NAME_BYTES]) -> Result<u32, ()> { todo!("lab-9: search_number") }
// TODO(lab-9): 传输有效目录项，容量/返回值按字节；保留 UserAddr 的页表复制边界。
pub fn transmit(_dir: &mut InodeGuard<'_>, _dst: super::inode::ReadDst<'_>) -> Result<usize, ()> { todo!("lab-9: transmit") }
// TODO(lab-9): resolve 相对路径从当前 cwd.dup 开始，绝对路径仍从根开始。
