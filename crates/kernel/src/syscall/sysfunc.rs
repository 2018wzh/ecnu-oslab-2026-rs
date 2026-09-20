use oslab_hal::arch::Syscall;
// TODO(lab-6): args[0] 为用户 NUL 字符串，经 copy_str_from_user 复制并有界打印，成功 0。
// 非法复制沿用 lab-5 panic，不构造用户引用。
pub fn print_str(_call: &Syscall) -> isize { todo!("lab-6: sys_print_str") }
// TODO(lab-6): args[0] 按有符号 i32 打印，成功返回 0。
pub fn print_int(_call: &Syscall) -> isize { todo!("lab-6: sys_print_int") }
