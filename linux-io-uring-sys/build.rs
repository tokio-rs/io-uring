#[cfg(not(feature = "bindgen"))]
fn main() {}

#[cfg(feature = "bindgen")]
fn main() {
    use std::env;
    use std::path::PathBuf;

    const INCLUDE: &str = r#"
#include <unistd.h>
#include <sys/syscall.h>
#include <linux/time_types.h>
#include <linux/io_uring.h>
    "#;

    #[cfg(not(feature = "overwrite"))]
    let outdir = PathBuf::from(env::var("OUT_DIR").unwrap());

    #[cfg(feature = "overwrite")]
    let outdir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");

    bindgen::Builder::default()
        .header_contents("include-file.h", INCLUDE)
        .ctypes_prefix("libc")
        .derive_default(true)
        .generate_comments(true)
        .use_core()
        .whitelist_type("io_uring_.*|io_.qring_.*|__kernel_timespec")
        .whitelist_var("__NR_io_uring.*|IOSQE_.*|IORING_.*")
        .generate()
        .unwrap()
        .write_to_file(outdir.join("sys.rs"))
        .unwrap();
}
