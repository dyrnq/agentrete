fn main() {
    println!("cargo:rustc-link-search=native=lib");
    println!("cargo:rustc-link-lib=static=sqlite_vec0");
    println!("cargo:rerun-if-changed=lib/libsqlite_vec0.a");
}
