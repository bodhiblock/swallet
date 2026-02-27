mod app;
mod config;
mod crypto;
mod error;
mod storage;
mod tui;

use app::App;
use config::AppConfig;

fn main() -> anyhow::Result<()> {
    // 加载配置
    let config = AppConfig::load_or_create()
        .map_err(|e| anyhow::anyhow!("配置加载失败: {e}"))?;

    // 初始化终端
    let mut terminal = tui::init()?;

    // 创建并运行应用
    let mut app = App::new(config);
    let result = app.run(&mut terminal);

    // 恢复终端
    tui::restore()?;

    result
}
