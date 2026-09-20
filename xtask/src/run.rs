use crate::{Result, build, config::Config, execute};
use std::process::Command;
pub fn run(c: &Config, debug: bool) -> Result<()> {
    if c.platform != "qemu-virt" {
        return Err("开发板请使用 image，然后通过 U-Boot bootm 启动".into());
    }
    let elf = build::kernel(c)?;
    crate::disk::ensure(c)?;
    let mut cmd = Command::new("qemu-system-riscv64");
    cmd.args(["-machine", "virt", "-bios", "default", "-m", "128M", "-smp"])
        .arg(c.ncpu.to_string())
        .args(["-nographic", "-kernel"])
        .arg(elf);
    cmd.args(["-global", "virtio-mmio.force-legacy=false", "-drive"])
        .arg(format!("file={},if=none,format=raw,id=x0", crate::disk::path(c).display()))
        .args(["-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0"]);
    if debug {
        cmd.args(["-S", "-gdb", "tcp::1234"]);
    }
    execute(&mut cmd)
}
