core::arch::global_asm!(include_str!("switch.S"));
#[repr(C)]
pub struct Context { pub ra: usize, pub sp: usize, pub saved: [usize; 12] }
const _: () = {
    assert!(core::mem::size_of::<Context>() == 112);
    assert!(core::mem::offset_of!(Context, saved) == 16);
};
impl Context { pub const ZERO: Self = Self { ra: 0, sp: 0, saved: [0; 12] }; }
unsafe extern "C" {
    /// # Safety
    /// 上下文有效且独占，next 栈及入口有效，切换期间关闭中断。
    pub fn arch_switch(old: *mut Context, next: *const Context);
}
