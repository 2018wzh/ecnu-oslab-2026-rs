use crate::{Result, build, config::Config, execute, root};
use std::process::Command;
pub fn image(c: &Config) -> Result<()> {
    let elf = build::kernel(c)?;
    let out = root().join("target").join(&c.name);
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    if !sysroot.status.success() {
        return Err("rustc --print sysroot 失败".into());
    }
    let host = Command::new("rustc").arg("-vV").output()?;
    let info = String::from_utf8(host.stdout)?;
    let host = info
        .lines()
        .find_map(|s| s.strip_prefix("host: "))
        .ok_or("缺少宿主三元组")?;
    let objcopy = std::path::PathBuf::from(String::from_utf8(sysroot.stdout)?.trim())
        .join(format!("lib/rustlib/{host}/bin/llvm-objcopy"));
    execute(
        Command::new(objcopy)
            .args(["-O", "binary"])
            .arg(&elf)
            .arg(out.join("kernel.bin")),
    )?;
    let its = format!(
        "/dts-v1/; / {{ description = \"OSLab\"; #address-cells = <1>; images {{ kernel {{ description = \"kernel\"; data = /incbin/(\"kernel.bin\"); type = \"kernel\"; arch = \"riscv\"; os = \"linux\"; compression = \"none\"; load = <{0:#x}>; entry = <{0:#x}>; }}; }}; configurations {{ default = \"conf\"; conf {{ kernel = \"kernel\"; }}; }}; }};",
        c.load
    );
    std::fs::write(out.join("kernel.its"), its)?;
    execute(Command::new("dtc").current_dir(&out).args([
        "-I",
        "dts",
        "-O",
        "dtb",
        "-o",
        "kernel.itb",
        "kernel.its",
    ]))?;
    println!("FIT: {}", out.join("kernel.itb").display());
    Ok(())
}
