// 发布构建时隐藏 macOS / Windows 的控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    worktree_manager_lib::run();
}
