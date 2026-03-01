mod app;
mod chain;
mod config;
mod crypto;
mod error;
mod multisig;
mod storage;
mod transfer;
mod tui;

use std::path::PathBuf;

use app::App;
use config::AppConfig;

anchor_lang::declare_program!(squads_multisig_program);

/// 解析命令行参数
fn parse_args() -> anyhow::Result<(Option<PathBuf>, Option<PathBuf>)> {
    let args: Vec<String> = std::env::args().collect();
    let mut data_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--load" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--load 需要指定钱包文件路径");
                }
                data_path = Some(PathBuf::from(&args[i]));
            }
            "--config" => {
                i += 1;
                if i >= args.len() {
                    anyhow::bail!("--config 需要指定配置文件路径");
                }
                config_path = Some(PathBuf::from(&args[i]));
            }
            "--help" | "-h" => {
                println!("swallet - Solana & Ethereum 钱包");
                println!();
                println!("用法: swallet [选项]");
                println!();
                println!("选项:");
                println!("  --load <路径>    指定钱包数据文件路径 (默认: ~/.config/swallet/data.dat)");
                println!("  --config <路径>  指定配置文件路径 (默认: ~/.config/swallet/config.toml)");
                println!("  -h, --help       显示帮助信息");
                std::process::exit(0);
            }
            other => {
                anyhow::bail!("未知参数: {other}\n使用 --help 查看帮助");
            }
        }
        i += 1;
    }

    Ok((data_path, config_path))
}

fn main() -> anyhow::Result<()> {
    // 解析命令行参数
    let (data_path, config_path) = parse_args()?;

    // 加载配置
    let config = AppConfig::load_or_create(config_path.as_deref())
        .map_err(|e| anyhow::anyhow!("配置加载失败: {e}"))?;

    // 初始化终端
    let mut terminal = tui::init()?;

    // 创建并运行应用
    let mut app = App::new(config, data_path);
    let result = app.run(&mut terminal);

    // 恢复终端
    tui::restore()?;

    result
}
