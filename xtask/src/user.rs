use crate::{Result, config::Config, execute, fit, root};
use std::{path::PathBuf, process::Command};
pub fn build(c: &Config) -> Result<PathBuf> {
    let out = root().join("target").join(&c.name);
    execute(
        Command::new("cargo")
            .current_dir(root())
            .args([
                "rustc",
                "--release",
                "-p",
                "oslab-user",
                "--bin",
                "init",
                "--target",
                &c.target,
                "--",
                "-C",
            ])
            .arg(format!(
                "link-arg=-T{}",
                root().join(&c.user_linker).display()
            ))
            .env("CARGO_TARGET_DIR", &out),
    )?;
    let elf = out.join(&c.target).join("release/init");
    let bin = out.join("init.bin");
    fit::binary(&elf, &bin)?;
    programs(c)?;
    Ok(bin)
}
pub fn programs(c: &Config) -> Result<Vec<PathBuf>> {
    let out = root().join("target").join(&c.name);
    let linker = root().join(&c.user_linker).with_file_name("elf.ld");
    let mut programs = Vec::new();
    for name in ["test_1", "test_2", "test_3", "test_4"] {
        execute(
            Command::new("cargo")
                .current_dir(root())
                .args([
                    "rustc",
                    "--release",
                    "-p",
                    "oslab-user",
                    "--bin",
                    name,
                    "--target",
                    &c.target,
                    "--",
                    "-C",
                ])
                .arg(format!("link-arg=-T{}", linker.display()))
                .args(["-C", "debuginfo=0", "-C", "strip=debuginfo"])
                .env("CARGO_TARGET_DIR", &out),
        )?;
        programs.push(out.join(&c.target).join("release").join(name));
    }
    Ok(programs)
}
