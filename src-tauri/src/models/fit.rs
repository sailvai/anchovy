//! Labels each model by how well it fits this Mac's memory.
//!
//! A model needs `min_ram_gb` to run at all. Only one model is loaded at a
//! time, but macOS and the user's other apps share the same memory, so a
//! model is Recommended only when the Mac has at least twice that.

use serde::Serialize;

pub const GIB: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Recommended,
    Fits,
    TooLarge,
}

pub fn fit(memory_bytes: u64, min_ram_gb: u32) -> Fit {
    let needed = u64::from(min_ram_gb) * GIB;
    if memory_bytes < needed {
        Fit::TooLarge
    } else if memory_bytes >= 2 * needed {
        Fit::Recommended
    } else {
        Fit::Fits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_the_given_memory_size() {
        let cases = [
            // (Mac memory in GB, model min_ram_gb, label)
            (8, 4, Fit::Recommended),
            (8, 8, Fit::Fits),
            (8, 16, Fit::TooLarge),
            (16, 4, Fit::Recommended),
            (16, 8, Fit::Recommended),
            (16, 16, Fit::Fits),
            (18, 16, Fit::Fits),
            (24, 16, Fit::Fits),
            (32, 16, Fit::Recommended),
            (8, 9, Fit::TooLarge),
        ];
        for (memory_gb, min_ram_gb, expected) in cases {
            assert_eq!(
                fit(memory_gb * GIB, min_ram_gb),
                expected,
                "{memory_gb} GB Mac, model needs {min_ram_gb} GB"
            );
        }
    }

    #[test]
    fn one_byte_short_is_too_large() {
        assert_eq!(fit(8 * GIB - 1, 8), Fit::TooLarge);
        assert_eq!(fit(16 * GIB - 1, 8), Fit::Fits);
    }

    #[test]
    fn labels_serialize_for_the_interface() {
        assert_eq!(
            serde_json::to_value(Fit::TooLarge).unwrap(),
            serde_json::json!("too_large")
        );
    }
}
