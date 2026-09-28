//! The two macOS facts the models module needs: how much memory this Mac has
//! and where models are stored. Kept thin; the logic lives in `fit.rs` and
//! `store.rs`.

use std::ffi::CStr;
use std::io;
use std::path::PathBuf;

/// Physical memory in bytes, from `sysctl hw.memsize`.
pub fn memory_bytes() -> io::Result<u64> {
    let name: &CStr = c"hw.memsize";
    let mut value: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `value` and `len` describe a valid u64 buffer, and hw.memsize
    // writes exactly one u64.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(value)
}

/// `~/Library/Application Support/Anchovy/Models`. Inside the app sandbox
/// `HOME` is the app's container, so this stays inside it.
pub fn models_dir() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/Anchovy/Models"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_same_memory_size_as_sysctl() {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .unwrap();
        let expected: u64 = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(memory_bytes().unwrap(), expected);
    }

    #[test]
    fn models_live_under_application_support() {
        let dir = models_dir().unwrap();
        assert!(dir.ends_with("Library/Application Support/Anchovy/Models"));
    }
}
