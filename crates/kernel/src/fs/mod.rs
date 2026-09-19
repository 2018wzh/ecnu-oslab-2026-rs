//! 文件系统: 把块设备上的一串扇区变成有名字、有层次、有元数据的对象。
//! 分 `inode` / `dir` / `dev` / `file` 等层, 每层只依赖它下面那一层, 并经过
//! `bio` 与 `bitmap` 访问磁盘。

// ---- 各子系统模块 ----
/// 把驱动层块设备适配成文件系统需要的接口。
pub mod bio;
pub mod adapter;
pub mod bitmap;
pub mod inode;
pub mod mount;
pub mod dir;
