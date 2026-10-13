fn main() {
    // sqlx::migrate! embeds the migrations at compile time
    println!("cargo:rerun-if-changed=migrations");
}
