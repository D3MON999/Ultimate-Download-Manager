// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started as the crash monitor of a running copy: watch it, then end.
    if udm_lib::crash_monitor() {
        return;
    }
    udm_lib::run()
}
