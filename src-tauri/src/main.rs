// VoiceKey – main.rs
// Thin binary entry point; delegates everything to the library crate.

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    voicekey_lib::run()
}
