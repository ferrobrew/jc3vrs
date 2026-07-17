//! Shared widgets for tabs that edit game memory through the patcher.

use crate::hooks;

/// A checkbox over a game bool, written through the patcher so pages that are not plainly writable
/// still take the edit (and so the write is reverted on uninject).
pub(super) fn patchbox(ui: &mut egui::Ui, label: &str, value: *mut bool) {
    let mut enabled = unsafe { *value };
    if ui.checkbox(&mut enabled, label).changed()
        && let Some(mut patcher) = hooks::patcher()
    {
        unsafe {
            patcher.patch(value as *const _ as usize, &[if enabled { 1 } else { 0 }]);
        }
    }
}

/// A slider over a game float global, written through the patcher like [`patchbox`] so pages that
/// are not plainly writable still take the edit.
pub(super) fn patch_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: *mut f32,
    range: std::ops::RangeInclusive<f32>,
) {
    let mut v = unsafe { *value };
    if ui
        .add(egui::Slider::new(&mut v, range).text(label))
        .changed()
        && let Some(mut patcher) = hooks::patcher()
    {
        unsafe {
            patcher.patch(value as *const _ as usize, &v.to_le_bytes());
        }
    }
}
