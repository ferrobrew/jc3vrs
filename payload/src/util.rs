use parking_lot::Mutex;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY};

pub fn is_pressed(key: VIRTUAL_KEY) -> bool {
    static LAST_INPUT: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    if LAST_INPUT
        .lock()
        .is_some_and(|last_input| last_input.elapsed() < std::time::Duration::from_millis(250))
    {
        return false;
    }

    let output = unsafe { GetAsyncKeyState(key.0 as _) != 0 };

    if output {
        *LAST_INPUT.lock() = Some(std::time::Instant::now());
    }

    output
}
