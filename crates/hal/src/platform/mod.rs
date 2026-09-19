#[cfg(all(feature = "qemu-virt", feature = "visionfive2"))]
compile_error!("只能选择一个平台");
#[cfg(not(any(feature = "qemu-virt", feature = "visionfive2")))]
compile_error!("必须选择平台");
#[cfg(feature = "qemu-virt")]
mod qemu_virt;
#[cfg(feature = "qemu-virt")]
pub use qemu_virt::*;
#[cfg(feature = "visionfive2")]
mod visionfive2;
#[cfg(feature = "visionfive2")]
pub use visionfive2::*;
