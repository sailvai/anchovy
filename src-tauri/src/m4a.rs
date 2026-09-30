//! M4A files: AAC through the macOS system codec, for Small quality. `mac`
//! holds the system calls; `recording::small` decides when to encode, and
//! `engines::audio` reads M4A for transcription.

pub mod mac;
