// `sqlx::migrate!` embeds the migrations at compile time: rebuild when one is
// added or changed, or a new migration ships in a binary that never runs it.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
