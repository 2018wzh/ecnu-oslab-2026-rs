use oslab_hal::arch::Syscall;
// TODO(lab-9): path, argv：最多 32 个参数，每个含 NUL 最多 128 字节；构造后提交，成功 argc，失败 -1。
pub fn exec(_call: &Syscall) -> isize { todo!("lab-9: exec") }
// TODO(lab-9): path, mode：CREATE=1/READ=2/WRITE=4；路径含 NUL 最多 128 字节；成功 fd，失败 -1。
pub fn open(_call: &Syscall) -> isize { todo!("lab-9: open") }
// TODO(lab-9): fd：成功 0，失败 -1。
pub fn close(_call: &Syscall) -> isize { todo!("lab-9: close") }
// TODO(lab-9): fd, len, addr：成功字节数，失败 0；用户地址须经页表复制。
pub fn read(_call: &Syscall) -> isize { todo!("lab-9: read") }
// TODO(lab-9): fd, len, addr：成功字节数，失败 0；用户地址须经页表复制。
pub fn write(_call: &Syscall) -> isize { todo!("lab-9: write") }
// TODO(lab-9): fd, unsigned offset, SET/ADD/SUB：尽力而为移动偏移，成功返回新偏移，失败 -1。
pub fn lseek(_call: &Syscall) -> isize { todo!("lab-9: lseek") }
// TODO(lab-9): fd：增加共享 file 引用，成功新 fd，失败 -1。
pub fn dup(_call: &Syscall) -> isize { todo!("lab-9: dup") }
// TODO(lab-9): fd, addr：复制 type:u16,nlink:u16,size:u32,inode_num:u32,offset:u32；成功 0，失败 -1。
pub fn fstat(_call: &Syscall) -> isize { todo!("lab-9: fstat") }
// TODO(lab-9): fd, addr, buffer_len：容量与返回值均为字节；传输有效项，失败 -1。
pub fn get_dentries(_call: &Syscall) -> isize { todo!("lab-9: get_dentries") }
// TODO(lab-9): path：成功 0，失败 -1。
pub fn mkdir(_call: &Syscall) -> isize { todo!("lab-9: mkdir") }
// TODO(lab-9): path：替换 cwd 引用；成功 0，失败 -1。
pub fn chdir(_call: &Syscall) -> isize { todo!("lab-9: chdir") }
// TODO(lab-9): 无参数：逆向构造路径后从返回偏移打印；成功 0，失败 -1。
pub fn print_cwd(_call: &Syscall) -> isize { todo!("lab-9: print_cwd") }
// TODO(lab-9): old_path,new_path：成功 0，失败 -1。
pub fn link(_call: &Syscall) -> isize { todo!("lab-9: link") }
// TODO(lab-9): path：成功 0，失败 -1。
pub fn unlink(_call: &Syscall) -> isize { todo!("lab-9: unlink") }

// sys_exec 参数暂存总量可达 4096 字节；在独立页面中保存并统一释放，不能把整组参数放进一页内核栈。
