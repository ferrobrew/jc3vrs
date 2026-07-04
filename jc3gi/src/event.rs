#![cfg_attr(any(), rustfmt::skip)]
pub const send_event_msg_impl_ADDRESS: usize = 0x1400719D0;
/// `NEvent::CSendEvent::SendMsg(const char*, void*)`: fires a named engine event to every
/// subscribed receiver, with an optional message payload (null for fire-and-forget). This is the
/// engine's own idiom for global events — `CDialogue::Play` fires `on_play_dialogue` through it —
/// and the sanctioned way to drive systems that subscribe named events in their `Init`, such as
/// the weather events `CWeatherController` subscribes (`weather_sunny`, `weather_rain`,
/// `weather_snow`, `weather_restore`, `weather_instant`, `cloud_base`, and `cloud_height`).
unsafe fn send_event_msg_impl(name: *const u8, data: *mut u8) {
    unsafe {
        let f: unsafe extern "system" fn(name: *const u8, data: *mut u8) = ::std::mem::transmute(
            send_event_msg_impl_ADDRESS,
        );
        f(name, data)
    }
}
/// Fire a named engine event with no payload.
pub fn send_event_msg(name: &std::ffi::CStr) {
    unsafe { send_event_msg_impl(name.as_ptr() as *const u8, std::ptr::null_mut()) }
}
