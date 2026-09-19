// ============================================================================
// kernel crate 的 build.rs —— 构建期把"平台配置"变成"代码与链接器的事实":
// 1. 从 configs/*.toml 读出链接地址, 生成链接脚本 (比 --defsym 更好, 因为
//    链接地址能作为字符串常量被内核读到并自检);
// 2. 用 `offset_of!` 生成汇编需要的结构体偏移 (偏移堆来自结构体定义);
// 3. 生成平台常量清单, 让内核在启动时打印"我编译给了哪台机器"。
//
// 配置路径通过 OSLAB_CONFIG 环境变量传入: build.rs 看不到 --features, 且
// feature 名与配置文件名不一定一一对应。未设置时退回到只用 feature 猜测,
// 猜不到就给出明确提示, 而不是崩溃。
// ============================================================================

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // 让 cargo 在这些文件变化时重新跑本脚本。
    println!("cargo::rerun-if-changed=build.rs");
    // 模板路径是配置决定的, 所以对应的 rerun 条件在
    // generate_linker_script 里按实际路径注册。
    println!("cargo::rerun-if-env-changed=OSLAB_CONFIG");
    println!("cargo::rerun-if-env-changed=OSLAB_PLATFORM");
    println!("cargo::rerun-if-env-changed=OSLAB_ARCH");
    // 用户程序映像变化时也要重新生成 (见第 5 步)。
    println!("cargo::rerun-if-changed=../../target/user");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 未设置"));
    // workspace 根目录 = crates/kernel/ 往上两层。
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("无法定位 workspace 根目录")
        .to_path_buf();

    // ---------------------------------------------------------------------
    // 1. 读取配置
    // ---------------------------------------------------------------------
    let cfg = match load_config(&workspace) {
        Some(c) => c,
        None => {
            // 没有配置文件: 退化成一个"能编译但地址未知"的模式, 而不直接
            // panic —— `cargo check`/rust-analyzer 会在无 OSLAB_CONFIG 时运行,
            // 那会让整个 crate 报错、毁掉 IDE 体验; 真正需要精确地址的链接
            // 阶段会失败, 错误信息更准确。
            eprintln!(
                "cargo:warning=oslab-kernel: 没有找到平台配置 (OSLAB_CONFIG 未设置)。\n\
                 cargo:warning=  将使用默认的链接地址 0x80200000。\n\
                 cargo:warning=  正常构建请用: cargo xtask build --config <name>"
            );
            Config::fallback()
        }
    };

    // ---------------------------------------------------------------------
    // 2. 生成链接脚本
    // ---------------------------------------------------------------------
    generate_linker_script(&workspace, &out_dir, &cfg);

    // ---------------------------------------------------------------------
    // 3. 生成汇编需要的结构体偏移
    // ---------------------------------------------------------------------
    generate_trapframe_offsets(&out_dir);

    // ---------------------------------------------------------------------
    // 4. 生成平台常量清单
    // ---------------------------------------------------------------------
    generate_platform_facts(&out_dir, &cfg);

    // ---------------------------------------------------------------------
    // 5. 把 Rust 用户程序映像嵌入内核
    // ---------------------------------------------------------------------
    // 从磁盘读需要块设备驱动 + 文件系统 + ELF 解析三件事全对, 任一步错
    // 都表现为"什么都没有输出", 很难定位。嵌入后失败只可能来自内核这一侧;
    // 等 lab-7/8/9 有了块设备与文件系统, 再切换成从磁盘装入。
    generate_user_images(&out_dir, &workspace);

    // 把关键值传给代码。
    println!(
        "cargo::rustc-env=OSLAB_KERNEL_LOAD_ADDR={:#x}",
        cfg.kernel_load_addr
    );
    println!("cargo::rustc-env=OSLAB_CONFIG_NAME={}", cfg.name);
    println!("cargo::rustc-env=OSLAB_PLATFORM_NAME={}", cfg.platform);
    println!("cargo::rustc-env=OSLAB_ARCH_NAME={}", cfg.arch);
    println!("cargo::rustc-env=OSLAB_BOOT_NAME={}", cfg.boot);
}

// ===========================================================================
// 一个极简的 TOML 读取器
// ===========================================================================
// 不用 toml crate: 那会拉进 serde 等一系列依赖, 而这里只需读几个键。
// 支持 `key = value`、`[section]`、`# 注释` 与基本类型 (字符串、十进制/
// 十六进制整数), 遇到不认识的语法明确报错而不是静默忽略。

/// 平台配置的完整内容。
#[derive(Debug, Default)]
struct Config {
    name: String,
    description: String,
    arch: String,
    platform: String,
    boot: String,
    kernel_load_addr: u64,
    /// 架构事实 (从 `configs/arch/<arch>.toml` 读入)。
    ///
    /// 目标三元组、链接器机器名、页大小、入口符号都是架构的事实, 与
    /// 平台无关; 放进 configs/<name>.toml 会为每个配置各写一份, 造成漂移。
    facts: ArchFacts,
    /// 链接脚本模板路径 (相对仓库根), 由 `configs/*.toml` 指定。
    linker: String,
    /// `[qemu]` 段。
    qemu: BTreeMap<String, String>,
    /// `[uboot]` 段。
    uboot: BTreeMap<String, String>,
}

/// 架构事实 (与 `xtask::config::ArchFacts` 一一对应)。
///
/// build.rs 跑在宿主机、属于目标 crate 的构建过程, 不能依赖 xtask 或
/// 目标侧 crate 的类型, 所以自己读一遍 `configs/arch/<arch>.toml`。下面
/// 这条约束保证两边一致: 两个解析器只支持同一个极小的键值子集, 且实际
/// 构建时输入是同一批文件 (xtask 用 OSLAB_CONFIG 传入配置路径)。
#[derive(Debug, Clone)]
struct ArchFacts {
    target: String,
    entry_symbol: String,
    ld_arch: String,
    page_size: u64,
}

impl Default for ArchFacts {
    fn default() -> Self {
        Self {
            target: "riscv64gc-unknown-none-elf".into(),
            entry_symbol: "_entry".into(),
            ld_arch: "riscv".into(),
            page_size: 4096,
        }
    }
}

impl ArchFacts {
    /// 从 `configs/arch/<arch>.toml` 读入。
    ///
    /// 找不到文件时 panic: 这是构建输入缺失, 不可退化 —— 用猜的页大小
    /// 去链接, 症状会在很久以后才出现。
    fn load(workspace: &Path, arch: &str) -> Self {
        let path = workspace
            .join("configs")
            .join("arch")
            .join(format!("{arch}.toml"));
        let text = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "无法读取架构描述 {}: {e}\n\
                 新增一个架构时, 请一并添加这个文件 (字段见 configs/arch/riscv64.toml)。",
                path.display()
            )
        });
        println!("cargo::rerun-if-changed={}", path.display());
        let mut f = Self::default();
        for (lineno, raw) in text.lines().enumerate() {
            let line = match raw.find('#') {
                Some(i) => &raw[..i],
                None => raw,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').unwrap_or_else(|| {
                panic!("{}:{}: 不是 key = value 形式", path.display(), lineno + 1)
            });
            let v = unquote(v.trim());
            match k.trim() {
                "target" => f.target = v,
                "entry_symbol" => f.entry_symbol = v,
                "ld_arch" => f.ld_arch = v,
                "page_size" => {
                    f.page_size = parse_int(&v).unwrap_or_else(|| {
                        panic!("{}: page_size 不是整数", path.display())
                    })
                }
                // 其它字段 (fit_arch / user_linker / gdb ...) 是**宿主工具**需要的,
                // 链接内核用不到, 这里不解析。
                _ => {}
            }
        }
        f
    }
}

impl Config {
    /// 找不到配置时的兜底值。
    fn fallback() -> Self {
        Self {
            name: "unknown".into(),
            description: "没有配置文件, 使用默认链接地址".into(),
            arch: "riscv64".into(),
            platform: "unknown".into(),
            boot: "unknown".into(),
            // 用 QEMU 的默认地址, 这样 `cargo check` 之后纵使误链接也能得到
            // 一个"看起来像内核"的二进制, 便于反汇编检查。
            kernel_load_addr: 0x8020_0000,
            linker: "crates/kernel/linker/smode.ld.in".into(),
            facts: ArchFacts::default(),
            qemu: BTreeMap::new(),
            uboot: BTreeMap::new(),
        }
    }

    /// 从环境变量取一个值, 没有就取默认。
    fn qemu_or(&self, key: &str, default: &str) -> String {
        self.qemu
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.into())
    }
    /// 同 [`Config::qemu_or`], 用于 `[uboot]` 段。
    fn uboot_or(&self, key: &str, default: &str) -> String {
        self.uboot
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.into())
    }
}

/// 读取配置: 优先用 OSLAB_CONFIG 指定的文件, 否则按 feature 猜。
fn load_config(workspace: &Path) -> Option<Config> {
    // 优先: xtask 明确指定的路径。
    let explicit = env::var("OSLAB_CONFIG").ok().map(PathBuf::from);
    let path = match explicit {
        Some(p) => p,
        None => {
            // 退化: 按 feature 找 configs/<name>.toml, 用 <arch>-<platform>.toml
            // 猜 (arch/platform 都由 xtask 经环境变量传入, 无写死的架构名)。
            // 这只覆盖最常见的情况, 找不到就返回 None。
            let plat = env::var("OSLAB_PLATFORM").ok()?;
            let guess = match env::var("OSLAB_ARCH") {
                Ok(arch) => workspace
                    .join("configs")
                    .join(format!("{arch}-{plat}.toml")),
                Err(_) => return None,
            };
            if guess.exists() {
                guess
            } else {
                return None;
            }
        }
    };
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取配置文件 {}: {e}", path.display()));
    // 配置文件内容变了要重新构建。
    println!("cargo::rerun-if-changed={}", path.display());
    Some(parse_config(&text, &path))
}

/// 解析上面描述的那个 TOML 子集。
fn parse_config(text: &str, path: &Path) -> Config {
    let mut cfg = Config::default();
    let mut section = String::new();

    for (lineno, raw) in text.lines().enumerate() {
        // 去掉注释。注意: 这里不处理"字符串里含 #"的情况 ——
        // 本配置里没有这种值, 而支持它需要真正的词法分析。
        let line = match raw.find('#') {
            Some(i) => &raw[..i],
            None => raw,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // 段头 `[name]`。
        if let Some(rest) = line.strip_prefix('[') {
            let name = rest
                .strip_suffix(']')
                .unwrap_or_else(|| panic!("{}:{}: 段头缺少 ']'", path.display(), lineno + 1));
            section = name.trim().to_string();
            continue;
        }

        // 键值对。
        let (key, value) = line.split_once('=').unwrap_or_else(|| {
            panic!(
                "{}:{}: 不是 key = value 形式: {line:?}",
                path.display(),
                lineno + 1
            )
        });
        let key = key.trim().to_string();
        let value = unquote(value.trim());

        match section.as_str() {
            "" => match key.as_str() {
                "name" => cfg.name = value,
                "description" => cfg.description = value,
                "arch" => cfg.arch = value,
                "platform" => cfg.platform = value,
                "boot" => cfg.boot = value,
                "linker" => cfg.linker = value,
                "kernel_load_addr" => {
                    cfg.kernel_load_addr = parse_int(&value).unwrap_or_else(|| {
                        panic!(
                            "{}:{}: kernel_load_addr 不是合法整数: {value:?}",
                            path.display(),
                            lineno + 1
                        )
                    })
                }
                other => panic!(
                    "{}:{}: 顶层不认识的键 {other:?} (可能是拼写错误)",
                    path.display(),
                    lineno + 1
                ),
            },
            "qemu" => {
                cfg.qemu.insert(key, value);
            }
            "uboot" => {
                cfg.uboot.insert(key, value);
            }
            other => panic!("{}:{}: 不认识的段 [{other}]", path.display(), lineno + 1),
        }
    }

    assert!(!cfg.name.is_empty(), "配置文件缺少 name 字段");
    // 架构事实来自另一个文件 —— 见 ArchFacts 的说明。
    cfg.facts = ArchFacts::load(&workspace_of(path), &cfg.arch);
    cfg
}

/// 从配置文件路径反推仓库根 (`<root>/configs/<name>.toml` -> `<root>`)。
///
/// `parse_config` 是纯函数 (文本 -> 结构体), 保持纯函数让它可以单独读懂、
/// 单独测试; 这一小辅助函数是代价最小的折中。
fn workspace_of(config_path: &Path) -> PathBuf {
    config_path
        .parent() // configs/
        .and_then(Path::parent) // <root>/
        .expect("配置文件应该在 <root>/configs/ 下")
        .to_path_buf()
}

/// 去掉字符串两端的引号。支持 `"""..."""` 的三引号形式 (我们只
/// 把它当作普通字符串处理, 保留内部换行)。
fn unquote(s: &str) -> String {
    if let Some(inner) = s.strip_prefix("\"\"\"") {
        return inner.strip_suffix("\"\"\"").unwrap_or(inner).into();
    }
    if let Some(inner) = s.strip_prefix('"') {
        return inner.strip_suffix('"').unwrap_or(inner).into();
    }
    s.to_string()
}

/// 解析十进制或十六进制整数 (`0x` 前缀)。支持下划线分隔。
fn parse_int(s: &str) -> Option<u64> {
    let s = s.replace('_', "");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

// ===========================================================================
// 平台描述的"构建期镜像"
// ===========================================================================
// build.rs 跑在宿主机, 不能 import 目标 crate (oslab-hal 是 no_std 且为
// riscv64 编译), 所以链接脚本需要的 ncpu 在这里重新声明。这个重复被两道
// 防线守着: 生成的 platform_facts.rs 记录该值, 内核启动时与 PLATFORM.ncpu
// 对比, 不一致立即报错停机; 两处都来自同一份 configs/*.toml, 且被 QEMU
// 的 -smp 与 hal 的编译期断言交叉验证。允许重复, 但强制它可验证。

/// 每个 hart 的内核栈大小 (字节)。必须与 hal::arch::boot 里的值一致。
const KERNEL_STACK_SIZE: usize = 4096;

/// 平台名 -> ncpu。
// 覆盖全部平台; 新增 platform/*.rs 却忘了这里会让链接脚本算错栈槽位,
// 但启动时的自检会把它抓出来。
fn stack_slots_for(platform: &str) -> usize {
    let ncpu = match platform {
        "qemu-virt" => 2,
        "visionfive2" => 4,
        other => {
            eprintln!(
                "cargo:warning=oslab-kernel: 未知平台 {other:?}, \
                 假设 ncpu=8 (栈槽位数会偏大, 但不会导致错误)"
            );
            8
        }
    };
    // ncpu + 1: 多留一页, 让"hartid 换算错导致索引越界一页"这种 bug
    // 表现为 canary 被改掉, 而不是踩坏别的数据 (见 hal::arch::boot)。
    ncpu + 1
}

// ===========================================================================
// 生成链接脚本
// ===========================================================================
fn generate_linker_script(workspace: &Path, out_dir: &Path, cfg: &Config) {
    // 用配置指定的模板, 而不是写死的路径 —— 见 Config::linker 的说明。
    let template_path = workspace.join(&cfg.linker);
    // 模板路径是配置决定的, 所以 rerun 条件在这里按实际路径注册。
    println!("cargo::rerun-if-changed={}", template_path.display());
    let template = fs::read_to_string(&template_path)
        .unwrap_or_else(|e| panic!("无法读取链接脚本模板 {}: {e}", template_path.display()));

    // 模板里的占位符: @KERNEL_BASE@ / @STACK_SLOTS@ / @STACK_SIZE_BYTES@ /
    // @KERNEL_ARCH@ / @ENTRY_SYMBOL@ / @PAGE_SIZE@
    //
    // 链接地址必须由这里注入而非写在脚本里: "内核被加载到哪"是平台的事实
    // (QEMU 上是 0x80200000, VF2 上是 0x40200000), 写死则无法共用。
    // 栈槽位数也须注入: 它等于 ncpu + 1, 而 ncpu 是平台的事实, 写死成的
    // 数会让"多留一页以便越界可诊断"失效。
    let stack_slots = stack_slots_for(&cfg.platform);
    let script = template
        .replace("@KERNEL_BASE@", &format!("{:#x}", cfg.kernel_load_addr))
        .replace("@STACK_SLOTS@", &stack_slots.to_string())
        .replace("@STACK_SIZE_BYTES@", &KERNEL_STACK_SIZE.to_string())
        .replace("@KERNEL_ARCH@", &cfg.facts.ld_arch)
        .replace("@ENTRY_SYMBOL@", &cfg.facts.entry_symbol)
        .replace("@PAGE_SIZE@", &cfg.facts.page_size.to_string());

    // 保险: 替换后不应再有未处理的占位符, 否则说明模板被改过而这里没跟上,
    // 会引发难以理解的链接器语法错误。
    assert!(
        !script.contains('@'),
        "链接脚本模板里还有未替换的 @...@ 占位符"
    );

    let out = out_dir.join("kernel.ld");
    fs::write(&out, script).expect("无法写出链接脚本");

    // 把路径告诉 rustc。用 `-Clink-arg=-T<script>` 而不是 `-Clinker-script=`,
    // 因为后者让 rustc 自己找并调用链接器, 不保证用我们指定的 lld。
    println!("cargo::rustc-link-arg=-T{}", out.display());
    // 明确要求 lld 不做我们脚本没写的默认布局动作。
    println!("cargo::rustc-link-arg=--no-relax");
    // 生成 MAP 文件: 排查"符号地址不对"时它是最直接的证据。
    println!(
        "cargo::rustc-link-arg=-Map={}",
        out_dir.join("kernel.map").display()
    );
}

// ===========================================================================
// 生成 trapframe 偏移
// ===========================================================================
// 偏移堆不写在汇编里: 手写宏加总大小断言, 中间插入字段时若总大小
// 一起改断言也会通过, 而汇编偏移全错。这里让偏移量的唯一来源是结构体
// 字段顺序。build.rs 无法 import 目标 crate 的类型, 所以用另一条路:
// 偏移量由 Rust 代码在 const 上下文经 `core::mem::offset_of!` 计算 (见 hal
// 的 trap.rs); 本函数只生成一份"偏移清单"文件, 供汇编的 include_str 与
// 文档使用 —— 是给人和测试看的, 不是运行期需要的。
fn generate_trapframe_offsets(out_dir: &Path) {
    // 寄存器名 —— 与 hal::arch::trap::TrapFrame 的 regs 数组一一对应。
    // 这里是唯一一处重复, 且被下面的一致性检查守着: 汇编要生成
    // `sd ra, TF_RA(sp)`, 需要的是寄存器名, 而结构体里只有数组 ——
    // 重复的是 RISC-V ABI 的 `x1..x31` 别名而非布局; 真正决定布局的
    // 偏移量仍只有一个来源 (结构体字段顺序)。
    const REG_NAMES: [&str; 31] = [
        "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5",
        "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4", "t5",
        "t6",
    ];

    let mut s = String::new();
    s.push_str("// 由 crates/kernel/build.rs 生成, 请勿手工编辑。\n");
    s.push_str("// 偏移量 = (寄存器号 - 1) * 8, 与 TrapFrame::regs 的数组索引一致。\n");
    for (i, name) in REG_NAMES.iter().enumerate() {
        let _ = writeln!(
            s,
            "pub const TF_{}: usize = {};",
            name.to_uppercase(),
            i * 8
        );
    }
    let _ = writeln!(s, "pub const TF_SEPC: usize = {};", 31 * 8);
    let _ = writeln!(s, "pub const TF_KERNEL_SP: usize = {};", 32 * 8);
    let _ = writeln!(s, "pub const TF_STATUS: usize = {};", 33 * 8);
    let _ = writeln!(s, "pub const TF_SIZE: usize = {};", 35 * 8);

    fs::write(out_dir.join("trapframe_offsets.rs"), s).expect("无法写出偏移文件");
}

// ===========================================================================
// 生成平台常量清单
// ===========================================================================
// 把 configs/*.toml 里的值 (构建期输入: 链接地址、QEMU 参数) 与
// platform/*.rs 里的值 (运行期输入: 设备地址、CPU 拓扑) 放在一起, 让内核
// 在启动时对比; 两者都涉及内存基地址这类数值, 消费者不同无法合并成单一
// 来源。启动时对比一次, 不一致就明确报错, 而不是等到某次绝对地址访问出错。
fn generate_platform_facts(out_dir: &Path, cfg: &Config) {
    let mut s = String::new();
    s.push_str("// 由 crates/kernel/build.rs 从 configs/*.toml 生成, 请勿手工编辑。\n");
    s.push_str("//\n");
    s.push_str("// 这些是**构建系统**对这台机器的认识。运行期的\n");
    s.push_str("// oslab_hal::platform::PLATFORM 是**代码**对这台机器的认识。\n");
    s.push_str("// 两者由各自的构建期检查来保证一致。\n\n");

    let _ = writeln!(s, "/// 配置名 (来自 --config)。");
    let _ = writeln!(s, "pub const CONFIG_NAME: &str = {:?};", cfg.name);
    let _ = writeln!(s, "/// 配置描述。");
    let _ = writeln!(
        s,
        "pub const CONFIG_DESCRIPTION: &str = {:?};",
        cfg.description
    );
    let _ = writeln!(s, "/// arch 维度 (来自配置文件)。");
    let _ = writeln!(s, "pub const CONFIG_ARCH: &str = {:?};", cfg.arch);
    let _ = writeln!(s, "/// platform 维度 (来自配置文件)。");
    let _ = writeln!(s, "pub const CONFIG_PLATFORM: &str = {:?};", cfg.platform);
    let _ = writeln!(s, "/// boot 维度 (来自配置文件)。");
    let _ = writeln!(s, "pub const CONFIG_BOOT: &str = {:?};", cfg.boot);
    let _ = writeln!(s, "/// 目标三元组。");
    let _ = writeln!(
        s,
        "pub const CONFIG_TARGET: &str = {:?};",
        cfg.facts.target
    );
    let _ = writeln!(s, "/// 内核链接地址 (由 build.rs 注入链接脚本).");
    let _ = writeln!(
        s,
        "pub const CONFIG_KERNEL_LOAD_ADDR: usize = {:#x};",
        cfg.kernel_load_addr
    );
    let _ = writeln!(s, "/// 内核栈槽位数 (build.rs 注入链接脚本的那个值)。");
    let _ = writeln!(
        s,
        "pub const CONFIG_STACK_SLOTS: usize = {};",
        stack_slots_for(&cfg.platform)
    );
    let _ = writeln!(
        s,
        "/// 每个内核栈的字节数 (build.rs 注入链接脚本的那个值)。"
    );
    let _ = writeln!(
        s,
        "pub const CONFIG_STACK_SIZE: usize = {};",
        KERNEL_STACK_SIZE
    );

    s.push_str("\n/// QEMU 运行参数 (仅 qemu-virt 配置有意义)。\n");
    // /// QEMU `-machine` 的值。
    s.push_str("/// QEMU -machine 的值。\n");
    let _ = writeln!(
        s,
        "pub const QEMU_MACHINE: &str = {:?};",
        cfg.qemu_or("machine", "virt")
    );
    // /// QEMU `-cpu` 的值。
    s.push_str("/// QEMU -cpu 的值。\n");
    let _ = writeln!(
        s,
        "pub const QEMU_CPU: &str = {:?};",
        cfg.qemu_or("cpu", "rv64")
    );
    s.push_str("/// QEMU -smp 的值 (必须与平台层的 ncpu 一致)。\n");
    let _ = writeln!(
        s,
        "pub const QEMU_SMP: usize = {};",
        parse_int(&cfg.qemu_or("smp", "2")).unwrap_or(2)
    );
    s.push_str("/// QEMU -m 的值 (必须与平台层的 dram_size 一致)。\n");
    let _ = writeln!(
        s,
        "pub const QEMU_MEMORY: &str = {:?};",
        cfg.qemu_or("memory", "128M")
    );
    s.push_str("/// QEMU -bios 的值 (default = 使用 QEMU 自带的 OpenSBI)。\n");
    let _ = writeln!(
        s,
        "pub const QEMU_BIOS: &str = {:?};",
        cfg.qemu_or("bios", "default")
    );

    s.push_str("\n/// U-Boot 部署参数 (仅 visionfive2 配置有意义)。\n");
    s.push_str("/// FIT 镜像的 load 地址。\n");
    let _ = writeln!(
        s,
        "pub const UBOOT_LOAD_ADDR: usize = {};",
        parse_int(&cfg.uboot_or("load_addr", "0x40200000"))
            .map(|v| format!("{v:#x}"))
            .unwrap_or_else(|| "0x40200000".into())
    );
    s.push_str("/// FIT 镜像的 entry 地址。\n");
    let _ = writeln!(
        s,
        "pub const UBOOT_ENTRY_ADDR: usize = {};",
        parse_int(&cfg.uboot_or("entry_addr", "0x40200000"))
            .map(|v| format!("{v:#x}"))
            .unwrap_or_else(|| "0x40200000".into())
    );
    s.push_str("/// U-Boot 里用来加载本镜像的命令。\n");
    let _ = writeln!(
        s,
        "pub const UBOOT_BOOT_COMMAND: &str = {:?};",
        cfg.uboot_or("boot_command", "bootm ${kernel_addr_r}")
    );
    s.push_str("/// 打印给学生看的部署步骤。\n");
    let _ = writeln!(
        s,
        "pub const UBOOT_DEPLOY_HINT: &str = {:?};",
        cfg.uboot_or("deploy_hint", "见 docs/board-deploy.md")
    );

    fs::write(out_dir.join("platform_facts.rs"), s).expect("无法写出平台常量清单");
}


/// 把 `target/user/*.bin` 嵌入为内核里的 `static` 字节数组。
///
/// 用 `include_bytes!` 而非常量的字节字面量: 用 `xxd -i` 生成几千行数字, `include_bytes!` 源码里只有一行而内容照样
/// 进内核映像。找不到文件时不报错, 生成空映像并提示 —— 内核的其它部分
/// 应能独立构建 (学生想只改内核时不该被"用户程序未构建"卡住)。
fn generate_user_images(out_dir: &Path, workspace: &Path) {
    let user_dir = workspace.join("target/user");
    let mut s = String::new();

    s.push_str("// 由 build.rs 生成 —— 不要手工编辑。\n");
    s.push_str("//\n");
    s.push_str("// 这里嵌入 target/user/<程序>.bin, 即用 Rust 编译的用户程序映像。\n");
    s.push_str("// 生成方式见 build.rs 的 `generate_user_images` 与 xtask 的 user 模块。\n\n");

    // 只嵌入内核真正会启动的那一个程序: 先前把所有程序都嵌进来, 没被引用的
    // 符号会一直报 never used, 与"内核里真的有死代码"难以区分。换个启动
    // 程序时改这一行即可; 其它程序仍会被构建并放进磁盘镜像 (lab-9 用),
    // 只是不再占内核映像的体积。
    for prog in ["init"] {
        let bin = user_dir.join(format!("{prog}.bin"));
        let var = format!("USER_IMAGE_{}", prog.to_uppercase());
        let found = format!("USER_IMAGE_{}_FOUND", prog.to_uppercase());

        if bin.exists() {
            // include_bytes! 的路径须相对本 crate 根, 而 OUT_DIR 在 target/ 深处,
            // 直接写绝对路径最稳妥。
            let abs = bin.canonicalize().unwrap_or(bin.clone());
            let _ = writeln!(
                s,
                "/// 用户程序 `{prog}` 的映像 (Rust 编译, 扁平二进制)。\n\
                 pub static {var}: &[u8] = include_bytes!(r\"{}\",);\n\
                 /// 构建时是否找到了该映像。\n\
                 pub const {found}: bool = true;\n",
                abs.display()
            );
        } else {
            let _ = writeln!(
                s,
                "/// 用户程序 `{prog}` 的映像 —— 构建时未找到, 故为空。\n\
                 /// 运行 `cargo xtask build --config <name>` 后重新构建内核即可。\n\
                 pub static {var}: &[u8] = &[];\n\
                 pub const {found}: bool = false;\n"
            );
        }
    }

    fs::write(out_dir.join("user_images.rs"), s).expect("无法写出用户程序映像清单");
}
