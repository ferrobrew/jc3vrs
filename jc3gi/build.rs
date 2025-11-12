use std::path::Path;

fn main() {
    pyxis::build_script(
        Path::new("pyxis-defs/projects/JustCause3/Steam/1227440"),
        Some(Path::new("src")),
    )
    .unwrap();
}
