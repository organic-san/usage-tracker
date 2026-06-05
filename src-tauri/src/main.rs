// 防止 Windows release 版彈出主控台視窗
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    usage_tracker_lib::run()
}
