fn main() {
    println!("cargo:rerun-if-changed=../../src/crates/concat/ui");
    slint_build::compile("preview.slint").expect("compile real Live Studio");
}
