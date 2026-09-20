/// 时钟创建：初始化各核心共享的系统时钟，仅在主核执行一次。
// TODO(lab-3): 初始化共享 ticks 及保护它的同步状态。
pub fn create() { todo!("lab-3: timer::create") }
/// 教师外围：读取 time，经 SBI TIME 设置本核的绝对截止时间。
pub fn init() {
    use oslab_hal::{arch::trap, platform::TIMER_INTERVAL};
    trap::set_timer(trap::time().wrapping_add(TIMER_INTERVAL)).expect("SBI timer init failed");
}
/// 全局系统时钟的更新；仅启动核调用。
// TODO(lab-3): 同步增加共享 ticks；不在此续订各核硬件定时器。
pub fn update() { todo!("lab-3: timer::update") }
/// 教师中断外围：每核续订，启动核记账。
/// 固件启动核不一定是 hart 0；SBI TIME 续订同时撤销当前定时器的 pending 状态。
pub fn tick() {
    init();
    if oslab_hal::arch::cpu::is_boot_cpu() { update(); }
}
// TODO(lab-3): 同步读取共享 tick 数。
pub fn ticks() -> usize { todo!("lab-3: timer::ticks") }
