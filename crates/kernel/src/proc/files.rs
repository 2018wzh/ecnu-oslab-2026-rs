use super::Proc;
use crate::fs::file::FileRef;
// TODO(lab-9): 首进程 cwd=root，stdin/stdout/stderr 为 fd0/1/2，失败回滚。
pub unsafe fn init(_p: *mut Proc) -> Result<(), ()> { todo!("lab-9: files::init") }
// TODO(lab-9): fork dup 每个文件及 cwd，不复制裸指针所有权，偏移共享。
pub unsafe fn clone(_parent: *const Proc, _child: *mut Proc) -> Result<(), ()> { todo!("lab-9: files::clone") }
// free 的两阶段回收见 lifecycle.rs；不在 exit 提前 Drop。
/// 构建 fd -> file 的映射，返回 fd（教师辅助）。
/// 成功转移一个引用，失败返回未消费的引用；独占借用保证文件表不并发修改。
pub fn fd_alloc(p: &mut Proc, file: FileRef) -> Result<usize, FileRef> {
    for (fd, slot) in p.files.iter_mut().enumerate() {
        if slot.is_none() { *slot = Some(file); return Ok(fd); }
    }
    Err(file)
}
/// 返回 fd 对应的文件（教师范围与空槽检查），不增加引用。
/// 参数解码留在架构边界；借用的生命周期不能超过进程文件表。
pub fn fd_get(p: &Proc, fd: usize) -> Option<&FileRef> { p.files.get(fd)?.as_ref() }

// SAFETY: init/clone 调用者独占资源字段，原始指针投影不覆盖正在借用的 lock；
// clone 时只持必要进程锁，失败移出引用后释放所有相关锁再 Drop。
