#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
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

    swallet_tauri_lib::run(data_path, config_path);
}
