fn main() {
    // Provider operation migrations belong to the runtime store.
    println!("cargo:rerun-if-changed=migrations");
}
