use crate::{Result, root};
use serde::Deserialize;

pub struct Config {
    pub name: String,
    pub platform: String,
    pub load: u64,
    pub ncpu: usize,
    pub target: String,
    pub linker: String,
    pub user_linker: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformConfig {
    arch: String,
    platform: String,
    load: u64,
    ncpu: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchConfig {
    target: String,
    linker: String,
    user_linker: String,
}

fn read<T: serde::de::DeserializeOwned>(relative: &str) -> Result<T> {
    let path = root().join(relative);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()).into())
}

pub fn load(name: &str) -> Result<Config> {
    if !matches!(name, "riscv64-qemu-virt" | "riscv64-visionfive2") {
        return Err("未知平台配置".into());
    }
    let platform: PlatformConfig = read(&format!("configs/{name}.toml"))?;
    if platform.arch != "riscv64" {
        return Err("当前仅支持 riscv64 架构".into());
    }
    let expected = match platform.platform.as_str() {
        "qemu-virt" => 2,
        "visionfive2" => 4,
        _ => return Err("未知 platform".into()),
    };
    if platform.ncpu != expected {
        return Err("ncpu 必须与 HAL 的平台常量一致".into());
    }
    let arch: ArchConfig = read(&format!("configs/arch/{}.toml", platform.arch))?;
    Ok(Config {
        name: name.into(),
        platform: platform.platform,
        load: platform.load,
        ncpu: platform.ncpu,
        target: arch.target,
        linker: arch.linker,
        user_linker: arch.user_linker,
    })
}
