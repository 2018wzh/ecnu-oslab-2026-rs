use std::{path::PathBuf, process::Command};
use crate::{config::Config, execute, fit, root, Result};
pub fn build(c: &Config) -> Result<PathBuf> {
    let out = root().join("target").join(&c.name);
    execute(Command::new("cargo").current_dir(root())
        .args(["rustc", "--release", "-p", "oslab-user", "--bin", "init", "--target", &c.target, "--", "-C"])
        .arg(format!("link-arg=-T{}", root().join(&c.user_linker).display()))
        .env("CARGO_TARGET_DIR", &out))?;
    let elf = out.join(&c.target).join("release/init");
    let bin = out.join("init.bin");
    fit::binary(&elf, &bin)?;
    Ok(bin)
}
