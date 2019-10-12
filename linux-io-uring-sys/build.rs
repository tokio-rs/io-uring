use std::env;
use std::path::PathBuf;

const INCLUDE: &str = r#"
#include <unistd.h>
#include <sys/syscall.h>
#include <linux/io_uring.h>
"#;

fn main() {
    let outdir = PathBuf::from(env::var("OUT_DIR").unwrap());

    bindgen::Builder::default()
        .header_contents("include-file.h", INCLUDE)
        .ctypes_prefix("libc")
        .generate_comments(true)
        .use_core()
        .whitelist_type("io_uring_.*|io_.qring_.*")
        .whitelist_var("__NR_io_uring.*|IOSQE_.*|IORING_.*")
        .generate().unwrap()
        .write_to_file(outdir.join("sys.rs")).unwrap();
}
