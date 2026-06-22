use std::path::Path;

fn main() {
    pyxis::build_script_with_options(
        Path::new("pyxis-defs/projects/JustCause3/Steam/20206564"),
        Some(Path::new("src")),
        pyxis::BuildOptions {
            public_addresses: true,
            ..Default::default()
        },
    )
    .unwrap();
}
