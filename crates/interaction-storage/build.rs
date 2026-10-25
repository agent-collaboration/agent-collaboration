fn main() {
    // Stable Rust must rerun migrate! when this owner's native schema changes.
    println!("cargo:rerun-if-changed=migrations");
}
