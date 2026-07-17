//! Embeds a compile-time build stamp so the running payload can prove which build it is. An
//! uninject can leave the module resident (a hung shutdown thread keeps the DLL mapped), and a
//! failed re-inject then silently continues on the stale code — the stamp is announced at startup
//! (log, in-headset banner, and the debug window) so a stale payload is visible at a glance.

fn main() {
    // Re-stamp whenever any payload source changes, so the stamp tracks the code it was built
    // from rather than the last time this script happened to run.
    println!("cargo:rerun-if-changed=src");
    let stamp = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
    println!("cargo:rustc-env=JC3VRS_BUILD_STAMP={stamp}");
}
