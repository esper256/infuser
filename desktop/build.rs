fn main() {
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_SHA");
}
