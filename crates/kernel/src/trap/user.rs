// TODO(lab-6): 时钟处理完成后，存在 Running 当前进程时 yield；恢复后保全原 PC/status。
// TODO(lab-4): 中断关闭时安装内核向量并确认来自 U-mode。
// trampoline 已保存 PC/status；保全返回状态，时钟/外部中断复用 lab-3。
// TODO(lab-5): U-mode ecall 用 HAL syscall::decode 后调用 crate::syscall::dispatch。
// 未知号由教师函数表报告调用号及 pid 后 panic。
// 识别完整异常号 13/15，读取 stval，调用 uvm::stack_grow；非法地址 panic。
// 成功增长后重试原指令，不推进 PC。
// 仅经 syscall::return_value 写返回值并推进 PC 一次；中断不推进 PC。
// 其他无法处理的陷阱报告原因、PC、stval 后 panic；最后 enter_user。
// SAFETY 要求：current/frame 有效且独占，不跨处理调用保留重叠可变借用。
#[unsafe(no_mangle)]
pub extern "C" fn user_trap() -> ! { todo!("lab-4: user_trap") }
// TODO(lab-4): 关闭中断，填写 frame 的内核 satp、虚拟栈顶、user_trap 和 hartid，
// 再调用 HAL return_to_user。返回准备仍是架构层学生任务。
// SAFETY 要求：当前进程独占 frame，页表及 trampoline 映射有效；进入汇编前结束借用。
pub fn enter_user() -> ! { todo!("lab-4: enter_user") }
