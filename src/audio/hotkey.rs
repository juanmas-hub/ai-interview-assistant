use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F9};
use std::time::Duration;
use std::thread;

pub type PauseFlag = Arc<AtomicBool>;

pub fn new_pause_flag() -> PauseFlag {
    Arc::new(AtomicBool::new(false))
}

pub fn spawn_hotkey_listener(flag: PauseFlag) {
    thread::Builder::new()
        .name("hotkey-listener".into())
        .spawn(move || run_hotkey_loop(flag))
        .expect("failed to spawn hotkey listener thread");
}

fn run_hotkey_loop(flag: PauseFlag) {
    println!("[hotkey] F9 activo — presioná para pausar / reanudar");

    let mut was_pressed = false;

    loop {
        let is_pressed = unsafe { GetAsyncKeyState(VK_F9 as i32) } < 0;

        if is_pressed && !was_pressed {
            let was_paused = flag.fetch_xor(true, Ordering::Relaxed);
            let label = if was_paused { "resumed ▶" } else { "paused  ⏸" };
            println!("[hotkey] Capture {label}");
        }

        was_pressed = is_pressed;
        thread::sleep(Duration::from_millis(30));
    }
}