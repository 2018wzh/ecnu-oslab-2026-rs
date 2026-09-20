pub mod user;
pub mod timer;
use oslab_hal::{arch::trap::TrapFrame, platform};
// SAFETY: PLIC 由当前平台提供；使用前先映射 MMIO。
pub static PLIC: oslab_drivers::irqchip::Plic = unsafe { oslab_drivers::irqchip::Plic::new(platform::PLIC_BASE) };
/// 初始化 trap 中各个核心共享的东西；教师提供初始化框架。
pub fn init() {
    PLIC.init(platform::UART_IRQ);
    timer::create();
    crate::console::enable_rx();
}
/// 初始化 trap 中各个核心独有的东西；所有依赖就绪后才打开中断。
pub fn init_hart() {
    use oslab_hal::arch::{cpu, csr, trap};
    trap::install_kernel_vector();
    PLIC.enable(platform::plic_context(cpu::hart_id()), platform::UART_IRQ);
    timer::init();
    trap::enable_sources();
    csr::irq_enable();
}
// 在 kernel_vector 中调用：内核态 trap 处理的核心逻辑。
// cause 是原因，frame.epc 是被打断的 PC，value 是随原因变化的附加信息。
// 先确认来源和中断状态，再区分中断、异常并分派；不要截断完整原因号。
// SAFETY: 汇编按 repr(C) 构造完整、对齐的栈帧，仅在调用期间独占借用。
// 不得把 frame 引用保留到返回之后。
#[unsafe(no_mangle)]
pub extern "C" fn kernel_trap(frame: &mut TrapFrame) {
    use oslab_hal::arch::{csr, trap};
    let (sepc, sstatus) = (frame.epc, frame.status);
    let (scause, stval) = (trap::cause(), trap::value());
    assert!(sstatus & (1 << 8) != 0, "kernel_trap: not from s-mode");
    assert!(!csr::irq_enabled(), "kernel_trap: interrupt enabled");
    // 只去掉中断标志，不截断原因号，也不用未经检查的原因号索引字符串表。
    let interrupt = 1usize << (usize::BITS - 1);
    let trap_id = scause & !interrupt;
    if scause & interrupt != 0 {
        match trap_id {
            // TODO(lab-3): 补充时钟和外部中断分支，处理后返回。
            _ => {
                crate::println!("unexpected interrupt: cause={:#x} sepc={:#x} stval={:#x}", trap_id, sepc, stval);
                panic!("kernel_trap");
            }
        }
    } else {
        match trap_id {
            // TODO(lab-3): 分析异常原因；不能处理的异常保留下面的诊断。
            _ => {
                crate::println!("unexpected exception: cause={:#x} sepc={:#x} stval={:#x}", trap_id, sepc, stval);
                panic!("kernel_trap");
            }
        }
    }
}
// TODO(lab-3): claim 并按设备分派，守卫离开作用域时 complete。
pub fn external_interrupt() { todo!("lab-3: external_interrupt") }
/// 教师读取循环：键盘输入 -> 屏幕输出。
/// 学生在 external_interrupt 中识别 UART 来源并调用这里。
pub fn uart_interrupt() {
    // TODO(lab-3): 在教师读取循环中补充换行和 Backspace 的回显处理。
    while let Some(c) = crate::console::getc() { crate::console::putc(c); }
}
