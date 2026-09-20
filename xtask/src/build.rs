use crate::{Result, config::Config, execute, root};
use std::{path::PathBuf, process::Command};
pub fn kernel(c: &Config) -> Result<PathBuf> {
    let user_image = crate::user::build(c)?;
    let out = root().join("target").join(&c.name);
    std::fs::create_dir_all(&out)?;
    let script = std::fs::read_to_string(root().join(&c.linker))?
        .replace("@LOAD@", &format!("{:#x}", c.load));
    let linker = out.join("kernel.ld");
    std::fs::write(&linker, script)?;
    execute(
        Command::new("cargo")
            .current_dir(root())
            .args([
                "build",
                "--release",
                "-p",
                "oslab-kernel",
                "--target",
                &c.target,
                "--features",
                &c.platform,
            ])
            .env("CARGO_TARGET_DIR", &out)
            .env("OSLAB_LINKER", &linker)
            .env("OSLAB_USER_IMAGE", &user_image),
    )?;
    let elf = out.join(&c.target).join("release/oslab-kernel");
    println!("ELF: {}", elf.display());
    Ok(elf)
}
