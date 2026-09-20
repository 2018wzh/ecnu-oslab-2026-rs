use core::sync::atomic::{AtomicBool, Ordering};
use crate::{lock::SpinLock, proc::schedule, mem::kvm};
use oslab_hal::{arch::cpu, platform};
use oslab_drivers::block::{Queue, VirtioBlock, Header};
static mut QUEUE: Queue = Queue::ZERO;
static mut DEVICE: Option<VirtioBlock> = None;
static LOCK: SpinLock = SpinLock::UNINIT;
static READY: AtomicBool = AtomicBool::new(false);
static mut ACTIVE: [bool; 8] = [false; 8];
static mut RESULT: [Option<Result<(), ()>>; 8] = [None; 8];
#[cfg(feature = "visionfive2")]
static mut SD: oslab_drivers::sd::Sd = oslab_drivers::sd::Sd::NEW;
/// 教师初始化；学生接入 main、页表、PLIC 和 trap。
pub fn init() -> Result<(), ()> {
    // SAFETY: 启动时独占，静态队列恒等映射且在所有请求结束前常驻。
    unsafe {
        LOCK.init();
        #[cfg(feature = "qemu-virt")]
        { let q = &raw mut QUEUE; DEVICE = Some(VirtioBlock::init(platform::BLOCK_BASE, q, q as usize, cpu::dma_fence)?); }
        #[cfg(feature = "visionfive2")]
        { (*(&raw mut SD)).init(platform::BLOCK_BASE, platform::CCACHE_BASE, cpu::dma_fence)?; }
    }
    READY.store(true, Ordering::Release); Ok(())
}
pub fn rw(block: u32, data: &mut [u8; super::BLOCK_SIZE], write: bool) -> Result<(), ()> {
    let pa = data.as_mut_ptr() as usize;
    if !READY.load(Ordering::Acquire) || block >= super::TOTAL_BLOCKS || pa % super::BLOCK_SIZE != 0
        || pa < platform::DRAM_BASE || pa > platform::DRAM_BASE + platform::DRAM_SIZE - super::BLOCK_SIZE { return Err(()); }
    // pin! 后不移动请求头；16 字节对齐保证不跨页。它只由设备读取。
    let header = core::pin::pin!(Header { kind: u32::from(write), reserved: 0, sector: u64::from(block) * 8 });
    let _header_pa = kvm::translate(header.as_ref().get_ref() as *const Header as usize);
    let mut guard = LOCK.lock();
    // SAFETY: 状态由 LOCK 保护。驱动可变借用仅限单次方法调用，绝不跨 sleep。
    // data 在 DMA 完成前无 CPU 访问；header 与 data 的拥有者都留在当前栈帧。
    unsafe {
        let id;
        loop {
            #[cfg(feature = "qemu-virt")]
            let submitted = (*(&raw mut DEVICE)).as_mut().unwrap().submit(_header_pa, pa, write);
            #[cfg(feature = "visionfive2")]
            let submitted = if ACTIVE[0] { None } else {
                (*(&raw mut SD)).submit(pa, block, write)?; Some(0)
            };
            if let Some(slot) = submitted { id = slot; break; }
            guard = schedule::sleep(&raw const ACTIVE as usize, guard);
        }
        ACTIVE[id] = true; RESULT[id] = None;
        let result = loop {
            if let Some(result) = RESULT[id] { break result; }
            guard = schedule::sleep((&raw const RESULT).cast::<Option<Result<(), ()>>>().add(id) as usize, guard);
        };
        #[cfg(feature = "qemu-virt")]
        (*(&raw mut DEVICE)).as_mut().unwrap().release(id);
        ACTIVE[id] = false;
        schedule::wakeup(&raw const ACTIVE as usize);
        drop(guard);
        result
    }
}
pub fn interrupt() {
    if !READY.load(Ordering::Acquire) { return; }
    let _guard = LOCK.lock();
    // SAFETY: 短借用；完成发布和逐请求唤醒在同一条件锁中。
    unsafe {
        loop {
            #[cfg(feature = "qemu-virt")]
            let completed = (*(&raw mut DEVICE)).as_mut().unwrap().complete();
            #[cfg(feature = "visionfive2")]
            let completed = (*(&raw mut SD)).complete().map(|result| (0, result));
            let Some((id, result)) = completed else { break; };
            assert!(ACTIVE[id] && RESULT[id].is_none());
            RESULT[id] = Some(result);
            schedule::wakeup((&raw const RESULT).cast::<Option<Result<(), ()>>>().add(id) as usize);
        }
    }
}
// TODO(lab-7): 映射平台块 MMIO；VF2 还需 CCACHE_BASE 16KiB RW 非用户映射。
pub fn map() { todo!("lab-7: block::map") }
