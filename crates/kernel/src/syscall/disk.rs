use oslab_hal::arch::Syscall;
// 参数读取、用户复制及算法接入由学生完成。地址令牌使用 fs::tokens 教师外围。
// TODO(lab-7): 申请 data block，返回绝对块号。
pub fn alloc_block(_call: &Syscall) -> isize { todo!("lab-7: sys_alloc_block") }
// TODO(lab-7): 归还绝对块号，成功返回 0。
pub fn free_block(_call: &Syscall) -> isize { todo!("lab-7: sys_free_block") }
// TODO(lab-7): 申请 inode，返回 inode 号，0 可分配。
pub fn alloc_inode(_call: &Syscall) -> isize { todo!("lab-7: sys_alloc_inode") }
// TODO(lab-7): 归还 inode 号，成功返回 0。
pub fn free_inode(_call: &Syscall) -> isize { todo!("lab-7: sys_free_inode") }
// TODO(lab-7): 选择 0=data、1=inode；成功 0，非法选择 -1。
pub fn show_bitmap(_call: &Syscall) -> isize { todo!("lab-7: sys_show_bitmap") }
// TODO(lab-7): 获取 buffer，成功返回内核地址令牌，失败 -1。
pub fn get_block(_call: &Syscall) -> isize { todo!("lab-7: sys_get_block") }
// TODO(lab-7): 将令牌对应 data 的完整 4096 字节复制到用户地址，成功 0。
pub fn read_block(_call: &Syscall) -> isize { todo!("lab-7: sys_read_block") }
// TODO(lab-7): 从用户地址复制完整 4096 字节到令牌对应 data，再写盘，成功 0。
pub fn write_block(_call: &Syscall) -> isize { todo!("lab-7: sys_write_block") }
// TODO(lab-7): 归还地址令牌且仅归还一次，成功 0。
pub fn put_block(_call: &Syscall) -> isize { todo!("lab-7: sys_put_block") }
// TODO(lab-7): 输出链表状态，成功 0。
pub fn show_buffer(_call: &Syscall) -> isize { todo!("lab-7: sys_show_buffer") }
// TODO(lab-7): 尝试释放 count 个非活跃数据页，成功返回 0（不是页数）。
pub fn flush_buffer(_call: &Syscall) -> isize { todo!("lab-7: sys_flush_buffer") }
