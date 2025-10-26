fn main() {
    pyxis::build_script(Some(std::path::Path::new("src"))).unwrap();
}
