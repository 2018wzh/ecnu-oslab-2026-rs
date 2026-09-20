use oslab_hal::arch::Syscall;
// TODO(lab-5): 校验地址、字节长度（长度非零，均页对齐、无溢出；地址为零或位于 mmap 区），非法返回 -1，后调用 uvm::mmap。
pub fn mmap(_call: &Syscall) -> isize { todo!("lab-5: sys_mmap") }
// TODO(lab-5): 校验非零页对齐长度、页对齐地址、无溢出及 mmap 区范围，非法 -1；解除后返回 0。
pub fn munmap(_call: &Syscall) -> isize { todo!("lab-5: sys_munmap") }
// TODO(lab-5): 0 查询；非零须页对齐且在 [0x2000, MMAP_BEGIN]，否则 -1。
// 比较新旧堆顶，分别组合 heap_grow / heap_ungrow，不变不分配，成功返回新堆顶。
pub fn brk(_call: &Syscall) -> isize { todo!("lab-5: sys_brk") }
