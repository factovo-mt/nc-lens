// リリース版で Windows のコンソール窓を出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nclens_app_lib::run()
}
