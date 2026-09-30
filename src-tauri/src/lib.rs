//! Tauri 装配层：注册插件与命令（S1 阶段命令表为空，S3 起逐个注册）。

pub mod atomic_json;
pub mod config;
pub mod copy;
pub mod error;
pub mod git;
pub mod manifest;
pub mod models;
pub mod path_utils;
pub mod platform;
pub mod validation;
pub mod vendor;

/// 启动应用。
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
