use oslab_hal::arch::Syscall;
// TODO(lab-5): 校验地址、字节长度（长度非零，均页对齐、无溢出；地址为零或位于 mmap 区），非法返回 -1，后调用 uvm::mmap。
pub fn mmap(_call: &Syscall) -> isize { todo!("lab-5: sys_mmap") }
// TODO(lab-5): 校验非零页对齐长度、页对齐地址、无溢出及 mmap 区范围，非法 -1；解除后返回 0。
pub fn munmap(_call: &Syscall) -> isize { todo!("lab-5: sys_munmap") }
// TODO(lab-5): 0 查询；非零须页对齐且在 [0x2000, MMAP_BEGIN]，否则 -1。
// 比较新旧堆顶，分别组合 heap_grow / heap_ungrow，不变不分配，成功返回新堆顶。
pub fn brk(_call: &Syscall) -> isize { todo!("lab-5: sys_brk") }
// 这三个调用只服务于 lab-5 测试，lab-6 移除。
// 用户地址只通过复制接口访问；非法复制输入可断言或 panic，不构造未经校验的用户引用。
// TODO(lab-5): args[0] 是 i32 数组，args[1] 是元素数；复制后打印，成功返回 0。
pub fn test_copyin(_call: &Syscall) -> isize { todo!("lab-5: sys_test_copyin") }
// TODO(lab-5): 将内核数组 [1i32,2,3,4,5] 复制到 args[0]，成功返回元素数 5。
pub fn test_copyout(_call: &Syscall) -> isize { todo!("lab-5: sys_test_copyout") }
// TODO(lab-5): 从 args[0] 复制 NUL 字符串到学生安排的内核缓冲（遵守 maxlen）并打印，成功返回 0。
pub fn test_copyinstr(_call: &Syscall) -> isize { todo!("lab-5: sys_test_copyinstr") }
