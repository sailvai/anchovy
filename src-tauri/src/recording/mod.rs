//! Recording: the selected microphone plus computer audio, mixed into one
//! 48 kHz mono file in the recording's folder.
//!
//! `core` holds the logic and is tested without audio hardware; `mixer` and
//! `file_writer` turn samples into the file; `small` turns it into an M4A for
//! Small quality; `mac` is the thin Core Audio layer; `commands` connects it
//! to the interface.

pub mod commands;
pub mod core;
pub mod file_writer;
pub mod mac;
pub mod mixer;
pub mod small;

#[cfg(test)]
mod tests {
    /// The sandboxed app may use the microphone, and says why it asks for the
    /// microphone and for computer audio (plan step 4a: no other entitlement
    /// is needed for the system-audio tap).
    #[test]
    fn the_bundle_asks_for_audio_input_and_explains_both_permissions() {
        let entitlements = include_str!("../../Entitlements.plist");
        assert!(entitlements.contains("<key>com.apple.security.app-sandbox</key>\n\t<true/>"));
        assert!(
            entitlements.contains("<key>com.apple.security.device.audio-input</key>\n\t<true/>")
        );

        let info = include_str!("../../Info.plist");
        for key in [
            "NSMicrophoneUsageDescription",
            "NSAudioCaptureUsageDescription",
        ] {
            let at = info
                .find(&format!("<key>{key}</key>"))
                .unwrap_or_else(|| panic!("{key}"));
            let rest = &info[at..];
            let text = &rest[rest.find("<string>").unwrap() + 8..rest.find("</string>").unwrap()];
            assert!(text.starts_with("Anchovy "), "{key}: {text}");
            assert!(
                text.ends_with('.') && text.matches(". ").count() == 0,
                "one sentence: {text}"
            );
        }
    }
}
