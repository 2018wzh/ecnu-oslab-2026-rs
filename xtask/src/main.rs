mod build;
mod config;
mod fit;
mod run;
mod user;
mod disk;
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    path::PathBuf,
    process::{Command, ExitCode},
};

#[derive(Parser)]
#[command(about = "OSLab 内核构建工具", arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Task,
    /// 目标平台配置
    #[arg(long, global = true, value_enum, default_value = "riscv64-qemu-virt")]
    config: ConfigName,
}

#[derive(Subcommand)]
enum Task {
    /// 编译内核
    Build,
    /// 编译并启动 QEMU
    Run,
    /// 启动 QEMU，等待 GDB 连接
    Debug,
    /// 生成 U-Boot FIT 镜像
    Image,
    /// 新建磁盘镜像，拒绝覆盖现有文件
    Disk { #[arg(long)] force: bool },
}

#[derive(Clone, Copy, ValueEnum)]
enum ConfigName {
    Riscv64QemuVirt,
    Riscv64Visionfive2,
}

impl ConfigName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Riscv64QemuVirt => "riscv64-qemu-virt",
            Self::Riscv64Visionfive2 => "riscv64-visionfive2",
        }
    }
}
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}
fn execute(cmd: &mut Command) -> Result<()> {
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("命令失败: {cmd:?}: {status}").into());
    }
    Ok(())
}
fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
fn dispatch(cli: Cli) -> Result<()> {
    let cfg = config::load(cli.config.as_str())?;
    match cli.command {
        Task::Build => {
            build::kernel(&cfg)?;
        }
        Task::Run => run::run(&cfg, false)?,
        Task::Debug => run::run(&cfg, true)?,
        Task::Image => fit::image(&cfg)?,
        Task::Disk { force } => disk::create(&cfg, force)?,
    }
    Ok(())
}
