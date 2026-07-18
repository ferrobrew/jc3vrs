//! Patches a vertex shader for single-pass stereo and writes the result to stdout.
//!
//! A dev tool for producing patched shaders to inspect (disassemble, diff, feed a validator):
//!
//! ```sh
//! cargo run -p dxbc-stereo --example patch --target x86_64-unknown-linux-gnu -- in.dxbc > out.dxbc
//! ```

use std::io::Write;

fn main() {
    let path = std::env::args().nth(1).expect("usage: patch <in.dxbc>");
    let blob = std::fs::read(&path).expect("read input");
    match dxbc_stereo::patch_vertex_shader(&blob) {
        Ok(patched) => std::io::stdout().write_all(&patched).expect("write stdout"),
        Err(e) => {
            eprintln!("patch: {e}");
            std::process::exit(1);
        }
    }
}
