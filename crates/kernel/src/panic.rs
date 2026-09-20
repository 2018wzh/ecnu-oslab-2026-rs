use core::fmt::{self, Write};
struct Emergency;
impl Write for Emergency {
    fn write_str(&mut self, s: &str) -> fmt::Result { crate::console::emergency(s); Ok(()) }
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    oslab_hal::arch::csr::irq_disable();
    let _ = writeln!(Emergency, "\nPANIC: {info}");
    oslab_hal::arch::cpu::park()
}
