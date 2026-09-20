//! 教师提供的行缓冲：中断生产，进程消费。可变引用不跨越 sleep。
use core::cell::UnsafeCell;
use crate::{console::putc, lock::SpinLock, proc::schedule};
struct Input { data: [u8; 128], read: usize, write: usize, edit: usize }
struct Shared(UnsafeCell<Input>);
// SAFETY: 所有访问由 LOCK 串行化；不保留跨解锁的引用。
unsafe impl Sync for Shared {}
static INPUT: Shared = Shared(UnsafeCell::new(Input { data: [0; 128], read: 0, write: 0, edit: 0 }));
static LOCK: SpinLock = SpinLock::UNINIT;
pub fn init() {
    // SAFETY: 启动时只调用一次，尚未启用 UART 接收中断。
    unsafe { LOCK.init(); }
}
pub fn edit(mut c: u8) {
    let _guard = LOCK.lock();
    // SAFETY: 持锁期间独占 input，wakeup 不访问此数据。
    unsafe {
        let p = &mut *INPUT.0.get();
        if c == 21 {
            while p.edit != p.write && p.data[p.edit.wrapping_sub(1) % 128] != b'\n' {
                p.edit = p.edit.wrapping_sub(1); putc(8); putc(b' '); putc(8);
            }
        } else if c == 8 || c == 127 {
            if p.edit != p.write { p.edit = p.edit.wrapping_sub(1); putc(8); putc(b' '); putc(8); }
        } else if c != 0 && p.edit.wrapping_sub(p.read) < 128 {
            if c == b'\r' { c = b'\n'; }
            if c != 4 { putc(c); }
            p.data[p.edit % 128] = c; p.edit = p.edit.wrapping_add(1);
            if c == b'\n' || c == 4 || p.edit.wrapping_sub(p.read) == 128 {
                p.write = p.edit; schedule::wakeup(INPUT.0.get() as usize);
            }
        }
    }
}
pub fn read(dst: &mut [u8]) -> usize {
    let mut guard = LOCK.lock(); let mut n = 0;
    while n < dst.len() {
        loop {
            // SAFETY: 持锁短读，不建立跨 sleep 的引用。
            let empty = unsafe { (*INPUT.0.get()).read == (*INPUT.0.get()).write };
            if !empty { break; }
            guard = schedule::sleep(INPUT.0.get() as usize, guard);
        }
        // SAFETY: 持锁且有数据，引用在此块结束，不跨睡眠。
        let c = unsafe {
            let p = &mut *INPUT.0.get(); let c = p.data[p.read % 128];
            p.read = p.read.wrapping_add(1);
            if c == 4 && n != 0 { p.read = p.read.wrapping_sub(1); } c
        };
        if c == 4 { break; }
        dst[n] = c; n += 1; if c == b'\n' { break; }
    }
    drop(guard); n
}
