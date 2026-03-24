#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    // 支持两种方式传参：
    // 1. 命令行: swallet --load /path/data.dat --config /path/config.toml
    // 2. 环境变量: SWALLET_DATA=/path/data.dat SWALLET_CONFIG=/path/config.toml
    let args: Vec<String> = std::env::args().collect();
    eprintln!("[main] args: {:?}", args);
    eprintln!("[main] SWALLET_DATA={:?}", std::env::var("SWALLET_DATA"));
    let mut data_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--load" => {
                i += 1;
                if i < args.len() { data_path = Some(PathBuf::from(&args[i])); }
            }
            "--config" => {
                i += 1;
                if i < args.len() { config_path = Some(PathBuf::from(&args[i])); }
            }
            _ => {}
        }
        i += 1;
    }

    // 环境变量覆盖
    if data_path.is_none() {
        if let Ok(p) = std::env::var("SWALLET_DATA") { data_path = Some(PathBuf::from(p)); }
    }
    if config_path.is_none() {
        if let Ok(p) = std::env::var("SWALLET_CONFIG") { config_path = Some(PathBuf::from(p)); }
    }

    eprintln!("[main] data_path={:?}, config_path={:?}", data_path, config_path);
    swallet_tauri_lib::run(data_path, config_path);
}
