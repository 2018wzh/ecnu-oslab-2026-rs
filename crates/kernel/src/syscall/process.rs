use oslab_hal::arch::Syscall;
// TODO(lab-6): 返回当前进程号。
pub fn getpid() -> isize { todo!("lab-6: sys_getpid") }
// TODO(lab-6): 参数转换后调用对应进程操作，保留负错误码。
pub fn fork() -> isize { todo!("lab-6: sys_fork") }
pub fn exit(_call: &Syscall) -> isize { todo!("lab-6: sys_exit") }
pub fn wait(_call: &Syscall) -> isize { todo!("lab-6: sys_wait") }
pub fn sleep(_call: &Syscall) -> isize { todo!("lab-6: sys_sleep") }
