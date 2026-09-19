//! `xtask::config` — 读取平台配置 (TOML 极小子集解析器)。
//! 解析失败时给出文件名、行号与"期望什么"的精确报错。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

// 读取配置时可能出现的错误。
#[derive(Debug)]
pub enum ConfigError {
    // `configs/` 目录不存在或无法读取。
    NoConfigDir(PathBuf, std::io::Error),
    // 指定的配置不存在。
    NotFound(PathBuf),
    // 语法错误 (带文件名与行号)。
    Syntax {
        // 出问题的文件。
        file: PathBuf,
        // 行号 (从 1 开始)。
        line: usize,
        // 说明。
        msg: String,
    },
    // 缺少必需字段。
    MissingField {
        // 出问题的文件。
        file: PathBuf,
        // 字段名。
        field: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::NoConfigDir(p, e) => {
                write!(f, "无法读取配置目录 {}: {e}", p.display())
            }
            ConfigError::NotFound(p) => {
                write!(f, "找不到配置文件 {}", p.display())
            }
            ConfigError::Syntax { file, line, msg } => {
                write!(f, "{}:{line}: {msg}", file.display())
            }
            ConfigError::MissingField { file, field } => {
                write!(f, "{}: 缺少必需字段 {field:?}", file.display())
            }
        }
    }
}

/// QEMU 运行参数。
#[derive(Debug, Clone)]
pub struct QemuOpts {
    /// `-machine` 的值 (例如 `virt`)。
    pub machine: String,
    /// `-cpu` 的值 (例如 `rv64`)。
    pub cpu: String,
    /// `-smp` 的值。
    pub smp: usize,
    /// `-m` 的值 (例如 `128M`)。
    pub memory: String,
    /// `-bios` 的值 (`default` 表示使用 QEMU 自带的 OpenSBI)。
    pub bios: String,
    /// `-display` 的值。
    pub display: String,
}

impl Default for QemuOpts {
    fn default() -> Self {
        Self {
            machine: "virt".into(),
            cpu: "rv64".into(),
            smp: 2,
            memory: "128M".into(),
            bios: "default".into(),
            display: "none".into(),
        }
    }
}

/// U-Boot 部署参数。
#[derive(Debug, Clone)]
pub struct UbootOpts {
    /// FIT 里的 load 地址。
    pub load_addr: u64,
    /// FIT 里的 entry 地址。
    pub entry_addr: u64,
    /// U-Boot 里用来加载本镜像的命令。
    pub boot_command: String,
    /// 打印给学生看的部署步骤。
    pub deploy_hint: String,
}

impl Default for UbootOpts {
    fn default() -> Self {
        Self {
            load_addr: 0x4020_0000,
            entry_addr: 0x4020_0000,
            boot_command: "bootm ${kernel_addr_r}".into(),
            deploy_hint: "见 docs/board-deploy.md".into(),
        }
    }
}

/// 一个平台配置的完整内容。
#[derive(Debug, Clone)]
pub struct Config {
    /// 配置名 (也是 `configs/<name>.toml` 的文件名)。
    pub name: String,
    /// 人类可读的描述。
    pub description: String,

    // ---- 三个正交的构建维度 ----
    /// CPU/ISA 语义层。
    pub arch: String,
    /// 机器语义层。
    pub platform: String,
    /// 启动路径语义层。
    pub boot: String,

    /// 内核链接地址 (= 加载地址)。
    pub kernel_load_addr: u64,
    /// 内核链接脚本模板, 相对仓库根。
    ///
    /// 段布局由启动协议决定 (S-mode 固件启动 / multiboot / UEFI 各不相同),
    /// 所以作为配置项而非写死。目前只有一份 S-mode 脚本。
    pub linker: String,

    /// 架构事实表 (从 `configs/arch/<arch>.toml` 读入, 不由配置档案提供)。
    ///
    /// 同一架构的各平台/启动方式必须共享同一份架构事实, 故独立成文件。
    pub facts: ArchFacts,

    /// QEMU 参数。
    pub qemu: QemuOpts,
    /// U-Boot 参数。
    pub uboot: UbootOpts,
}

impl Config {
    /// 这个配置是否用 QEMU 运行 (而不是交付给真实开发板)。
    pub fn is_qemu(&self) -> bool {
        self.platform == "qemu-virt"
    }

    /// 目标 crate 需要的 cargo feature。
    ///
    /// `arch` / `platform` 两维度各变成一个 `--features <维度>-<值>`,
    /// 选择发生在编译期, 由 cargo 的 feature 机制实现。
    ///
    /// `boot` 维度不是 feature: 当前两个启动协议 (OpenSBI / U-Boot)
    /// 走同一段代码 (入口 ABI 相同, 仅交付格式与运行方式不同), 由
    /// `linker` 字段与 [qemu]/[uboot] 段表达。代码形状不同的启动协议
    /// (裸机直启、multiboot2、UEFI) 才需要 `boot-*` feature。
    pub fn cargo_features(&self) -> String {
        format!("arch-{},platform-{}", self.arch, self.platform)
    }

    /// 目标三元组 (来自 `configs/arch/<arch>.toml` 的架构事实)。
    pub fn target(&self) -> &str {
        &self.facts.target
    }

    /// 该配置的构建产物目录。
    pub fn artifact_dir_name(&self) -> String {
        self.name.clone()
    }
}


/// 一个架构的"事实表"。
///
/// 单独放在 `configs/arch/<arch>.toml` 里: 换架构只改这一处文件。
#[derive(Debug, Clone)]
pub struct ArchFacts {
    /// 目标三元组 (`rustc --target`)。
    pub target: String,
    /// U-Boot FIT 里的 `arch` 属性 (bootm 会校验)。
    pub fit_arch: String,
    /// QEMU 系统模拟器名。
    pub qemu_system: String,
    /// GDB 可执行文件名。
    pub gdb: String,
    /// GDB 的架构名 (`set architecture ...`)。
    pub gdb_arch: String,
    /// 页大小 (链接脚本对齐粒度)。
    pub page_size: u64,
    /// 内核入口符号 (内核链接脚本的 ENTRY 与镜像入口点)。
    pub entry_symbol: String,
    /// `OUTPUT_ARCH(...)` 的值 (binutils 的机器名, 如 `riscv`)。
    pub ld_arch: String,
    /// 用户程序的链接基址 (由内核装入方式决定)。
    pub user_base: u64,
    /// 用户程序链接脚本模板, 相对仓库根。
    pub user_linker: String,
}

/// 读取某个架构的描述文件。找不到时给出明确的、可执行的错误。
pub fn load_arch(arch: &str) -> Result<ArchFacts, ConfigError> {
    let path = configs_dir().join("arch").join(format!("{arch}.toml"));
    if !path.exists() {
        return Err(ConfigError::Syntax {
            file: path.clone(),
            line: 0,
            msg: format!(
                "缺少架构描述文件 {}。新增一个架构时, 请一并添加它 \
                 (字段见 configs/arch/riscv64.toml)。",
                path.display()
            ),
        });
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| ConfigError::NoConfigDir(path.clone(), e))?;
    let top = parse_top_level(&text, &path)?;

    let get = |k: &str| -> Result<String, ConfigError> {
        top.get(k).cloned().ok_or_else(|| ConfigError::Syntax {
            file: path.clone(),
            line: 0,
            msg: format!("架构描述缺少字段 {k:?}"),
        })
    };
    let int = |k: &str| -> Result<u64, ConfigError> {
        get(k)?
            .parse::<u64>()
            .map_err(|_| ConfigError::Syntax {
                file: path.clone(),
                line: 0,
                msg: format!("{k} 不是整数"),
            })
    };
    Ok(ArchFacts {
        target: get("target")?,
        fit_arch: get("fit_arch")?,
        qemu_system: get("qemu_system")?,
        gdb: get("gdb")?,
        gdb_arch: get("gdb_arch")?,
        page_size: int("page_size")?,
        entry_symbol: get("entry_symbol")?,
        ld_arch: get("ld_arch")?,
        user_base: crate::config::parse_int(&get("user_base")?).ok_or_else(|| {
            ConfigError::Syntax {
                file: path.clone(),
                line: 0,
                msg: "user_base 不是合法整数".into(),
            }
        })?,
        user_linker: get("user_linker")?,
    })
}

/// 解析一个"只有顶层键值对"的配置文件 (架构描述就是这种)。
fn parse_top_level(
    text: &str,
    file: &Path,
) -> Result<BTreeMap<String, String>, ConfigError> {
    let mut out = BTreeMap::new();
    for (i, raw) in text.lines().enumerate() {
        let line = match raw.find('#') {
            Some(idx) => &raw[..idx],
            None => raw,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| ConfigError::Syntax {
            file: file.into(),
            line: i + 1,
            msg: format!("不是 key = value 形式: {line:?}"),
        })?;
        out.insert(k.trim().to_string(), unquote(v.trim()));
    }
    Ok(out)
}

/// `configs/` 目录的路径。
fn configs_dir() -> PathBuf {
    // xtask 的 manifest 目录是 <root>/xtask, 所以往上一层就是仓库根。
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("xtask 应该在仓库根目录的子目录里")
        .join("configs")
}

/// 读取指定名字的配置。
pub fn load(name: &str) -> Result<Config, ConfigError> {
    let path = configs_dir().join(format!("{name}.toml"));
    if !path.exists() {
        return Err(ConfigError::NotFound(path));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| ConfigError::NoConfigDir(path.clone(), e))?;
    let mut cfg = parse(&text, &path)?;
    cfg.facts = load_arch(&cfg.arch)?;
    Ok(cfg)
}

/// 读取 `configs/` 下所有配置, 按名字排序。
pub fn load_all() -> Result<Vec<Config>, ConfigError> {
    let dir = configs_dir();
    let entries = std::fs::read_dir(&dir).map_err(|e| ConfigError::NoConfigDir(dir.clone(), e))?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let text =
            std::fs::read_to_string(&p).map_err(|e| ConfigError::NoConfigDir(p.clone(), e))?;
        let mut cfg = parse(&text, &p)?;
        // 架构事实也要填上 —— 否则 `cargo xtask list -v` 之类的只读
        // 路径会打印出空的三元组。
        cfg.facts = load_arch(&cfg.arch)?;
        out.push(cfg);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 解析配置文件 (TOML 的一个极小子集)。
fn parse(text: &str, file: &Path) -> Result<Config, ConfigError> {
    let mut top: BTreeMap<String, String> = BTreeMap::new();
    let mut qemu: BTreeMap<String, String> = BTreeMap::new();
    let mut uboot: BTreeMap<String, String> = BTreeMap::new();
    let mut section = Section::Top;

    for (i, raw) in text.lines().enumerate() {
        let lineno = i + 1;
        // 去注释。不处理"字符串里含 #"的情况 —— 本配置里没有这种值,
        // 而支持它需要真正的词法分析 (见文件顶部的说明)。
        let line = match raw.find('#') {
            Some(idx) => &raw[..idx],
            None => raw,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // 段头。
        if let Some(rest) = line.strip_prefix('[') {
            let name = rest.strip_suffix(']').ok_or_else(|| ConfigError::Syntax {
                file: file.into(),
                line: lineno,
                msg: format!("段头缺少 ']': {line:?}"),
            })?;
            section = match name.trim() {
                "qemu" => Section::Qemu,
                "uboot" => Section::Uboot,
                other => {
                    return Err(ConfigError::Syntax {
                        file: file.into(),
                        line: lineno,
                        msg: format!(
                            "不认识的段 [{other}]。本工具认识 [qemu] 与 [uboot]。\
                             是不是想写 [qemu]?"
                        ),
                    })
                }
            };
            continue;
        }

        // 键值对。
        let (key, value) = line.split_once('=').ok_or_else(|| ConfigError::Syntax {
            file: file.into(),
            line: lineno,
            msg: format!("不是 key = value 形式: {line:?}"),
        })?;
        let key = key.trim().to_string();
        let value = unquote(value.trim());

        let table = match section {
            Section::Top => &mut top,
            Section::Qemu => &mut qemu,
            Section::Uboot => &mut uboot,
        };
        // 重复键报错而不是静默覆盖: 那通常是复制粘贴事故。
        if table.insert(key.clone(), value.clone()).is_some() {
            return Err(ConfigError::Syntax {
                file: file.into(),
                line: lineno,
                msg: format!("键 {key:?} 重复定义"),
            });
        }
    }

    // ---- 必需字段 ----
    let get = |k: &'static str| -> Result<String, ConfigError> {
        top.get(k).cloned().ok_or(ConfigError::MissingField {
            file: file.into(),
            field: k,
        })
    };

    // 顶层键必须**全部**被认识: 拼错的键会被静默忽略, 直接报错并点名。
    const KNOWN_TOP_KEYS: [&str; 7] = [
        "name",
        "description",
        "arch",
        "platform",
        "boot",
        "linker",
        "kernel_load_addr",
    ];
    for k in top.keys() {
        if !KNOWN_TOP_KEYS.contains(&k.as_str()) {
            return Err(ConfigError::Syntax {
                file: file.into(),
                line: 0,
                msg: format!(
                    "顶层不认识的键 {k:?}。已知的键: {}。\
                     (注意: 目标三元组不是配置档案的字段, \
                     它来自 configs/arch/<arch>.toml)",
                    KNOWN_TOP_KEYS.join(", ")
                ),
            });
        }
    }

    let name = get("name")?;
    // arch/platform/boot 三个维度是必需的: 它们决定了编译给谁、
    // 用哪个 feature、以及怎么交付。缺任何一个都说明配置不完整。
    let arch = get("arch")?;
    let platform = get("platform")?;
    let boot = get("boot")?;
    // 链接脚本模板。缺省时落到 S-mode 交接的标准布局 ——
    // 这样缺这一行的配置文件仍然能构建, 而需要特殊布局的
    // bootloader 必须显式写出来 (隐式会让人以为"随便哪个脚本都行")。
    let linker = top
        .get("linker")
        .cloned()
        .unwrap_or_else(|| "crates/kernel/linker/smode.ld.in".to_string());

    let kernel_load_addr =
        parse_int(&get("kernel_load_addr")?).ok_or_else(|| ConfigError::Syntax {
            file: file.into(),
            line: 0,
            msg: "kernel_load_addr 不是合法的整数 (支持 0x 十六进制与十进制, 可用下划线)".into(),
        })?;

    // ---- 可选字段 ----
    let description = top.get("description").cloned().unwrap_or_default();

    let mut q = QemuOpts::default();
    if let Some(v) = qemu.get("machine") {
        q.machine = v.clone();
    }
    if let Some(v) = qemu.get("cpu") {
        q.cpu = v.clone();
    }
    if let Some(v) = qemu.get("smp") {
        q.smp = parse_int(v).unwrap_or(2) as usize;
    }
    if let Some(v) = qemu.get("memory") {
        q.memory = v.clone();
    }
    if let Some(v) = qemu.get("bios") {
        q.bios = v.clone();
    }
    if let Some(v) = qemu.get("display") {
        q.display = v.clone();
    }

    let mut u = UbootOpts::default();
    if let Some(v) = uboot.get("load_addr") {
        u.load_addr = parse_int(v).unwrap_or(u.load_addr);
    }
    if let Some(v) = uboot.get("entry_addr") {
        u.entry_addr = parse_int(v).unwrap_or(u.entry_addr);
    }
    if let Some(v) = uboot.get("boot_command") {
        u.boot_command = v.clone();
    }
    if let Some(v) = uboot.get("deploy_hint") {
        u.deploy_hint = v.clone();
    }

    Ok(Config {
        // facts 由 load() 填入; 直接 parse 出来时是空壳 (单测用)。
        facts: ArchFacts {
            target: String::new(),
            fit_arch: String::new(),
            qemu_system: String::new(),
            gdb: String::new(),
            gdb_arch: String::new(),
            page_size: 4096,
            entry_symbol: String::new(),
            ld_arch: String::new(),
            user_base: 0x1000,
            user_linker: String::new(),
        },
        name,
        description,
        arch,
        platform,
        boot,
        kernel_load_addr,
        linker,
        qemu: q,
        uboot: u,
    })
}

/// 当前所在的段。
enum Section {
    Top,
    Qemu,
    Uboot,
}

/// 去掉字符串两端的引号, 包含 `"""` 三引号形式。
fn unquote(s: &str) -> String {
    if let Some(inner) = s.strip_prefix("\"\"\"") {
        return inner.strip_suffix("\"\"\"").unwrap_or(inner).to_string();
    }
    if let Some(inner) = s.strip_prefix('"') {
        return inner.strip_suffix('"').unwrap_or(inner).to_string();
    }
    // 没引号: 当作裸值 (数字或裸字符串)。
    s.to_string()
}

/// 解析十进制或十六进制整数。支持下划线分组 (`0x8020_0000`)。
pub fn parse_int(s: &str) -> Option<u64> {
    let cleaned = s.trim().replace('_', "");
    if let Some(hex) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        cleaned.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> Config {
        parse(s, &PathBuf::from("test.toml")).expect("应该解析成功")
    }

    #[test]
    fn parses_minimal_config() {
        let c = p(r#"
name = "riscv64-qemu-virt"
arch = "riscv64"
platform = "qemu-virt"
boot = "sbi"
kernel_load_addr = 0x8020_0000
"#);
        assert_eq!(c.name, "riscv64-qemu-virt");
        assert_eq!(c.kernel_load_addr, 0x8020_0000);
        assert!(c.is_qemu());
        // 三个维度各自变成一个 feature —— 目标三元组不在其中, 它来自
        // configs/arch/<arch>.toml。
        assert_eq!(c.cargo_features(), "arch-riscv64,platform-qemu-virt");
    }

    #[test]
    fn loads_arch_facts_separately_from_profile() {
        // 配置档案里**不应该**再有 target: 那是架构事实。
        let c = crate::config::load("riscv64-qemu-virt").expect("应该能读到配置");
        assert_eq!(c.target(), "riscv64gc-unknown-none-elf");
        assert_eq!(c.facts.fit_arch, "riscv");
        assert_eq!(c.facts.page_size, 4096);
    }

    #[test]
    fn parses_sections_and_ignores_comments() {
        let c = p(r#"
# 这是注释
name = "riscv64-visionfive2"
arch = "riscv64"        # 行尾注释
platform = "visionfive2"
boot = "uboot"
kernel_load_addr = 0x40200000

[uboot]
load_addr = 0x40200000
entry_addr = 0x40200000
boot_command = "bootm ${kernel_addr_r}"
"#);
        assert!(!c.is_qemu());
        assert_eq!(c.uboot.load_addr, 0x4020_0000);
        assert_eq!(c.uboot.boot_command, "bootm ${kernel_addr_r}");
    }

    #[test]
    fn missing_field_is_reported() {
        let e = parse("name = \"x\"", &PathBuf::from("t.toml")).unwrap_err();
        match e {
            ConfigError::MissingField { field, .. } => assert_eq!(field, "arch"),
            other => panic!("期望 MissingField, 得到 {other:?}"),
        }
    }

    #[test]
    fn duplicate_key_is_reported() {
        let e = parse("name = \"a\"\nname = \"b\"", &PathBuf::from("t.toml")).unwrap_err();
        assert!(matches!(e, ConfigError::Syntax { line: 2, .. }));
    }

    #[test]
    fn parse_int_supports_hex_and_underscores() {
        assert_eq!(parse_int("0x8020_0000"), Some(0x8020_0000));
        assert_eq!(parse_int("128"), Some(128));
        assert_eq!(parse_int("0X10"), Some(16));
        assert_eq!(parse_int("nope"), None);
    }
}
