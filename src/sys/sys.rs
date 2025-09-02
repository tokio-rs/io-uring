cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        include!("sys_x86_64.rs");
    } else if #[cfg(target_arch = "x86")] {
        include!("sys_x86.rs");
    } else if #[cfg(target_arch = "arm")] {
        include!("sys_arm.rs");
    } else if #[cfg(target_arch = "aarch64")] {
        include!("sys_aarch64.rs");
    } else if #[cfg(target_arch = "riscv64")] {
        include!("sys_riscv64.rs");
    } else if #[cfg(target_arch = "powerpc")] {
        include!("sys_powerpc.rs");
    } else if #[cfg(target_arch = "powerpc64")] {
        include!("sys_powerpc64.rs");
    } else if #[cfg(target_arch = "loongarch64")] {
        include!("sys_loongarch64.rs");
    } else if #[cfg(target_arch = "s390x")] {
        include!("sys_s390x.rs");
    } else {
        include!("sys_x86_64.rs");
    }
}
