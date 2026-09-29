//! The macOS calls for first launch: the microphone permission through
//! AVFoundation, and opening the Privacy pane of System Settings. Kept thin;
//! the logic lives in `setup.rs`.

use std::sync::mpsc;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_app_kit::NSWorkspace;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_foundation::{NSString, NSURL};

use super::Access;

/// The System Settings panes a denied permission is changed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyPane {
    Microphone,
    /// Screen & System Audio Recording, which also holds System Audio
    /// Recording Only.
    ComputerAudio,
}

impl PrivacyPane {
    pub fn url(self) -> &'static str {
        match self {
            PrivacyPane::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            PrivacyPane::ComputerAudio => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        }
    }
}

fn access(status: AVAuthorizationStatus) -> Access {
    match status {
        AVAuthorizationStatus::Authorized => Access::Allowed,
        AVAuthorizationStatus::NotDetermined => Access::NotAsked,
        // Restricted: a profile or parental control forbids it.
        _ => Access::Denied,
    }
}

/// The microphone permission, without asking.
pub fn microphone_access() -> Access {
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return Access::Denied;
    };
    // SAFETY: AVMediaTypeAudio is a valid media type for this call.
    access(unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) })
}

/// Shows the system microphone prompt if it has not been shown, and waits
/// for the answer. Call off the main thread.
pub fn request_microphone() -> Access {
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return Access::Denied;
    };
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool| {
        let _ = tx.send(granted.as_bool());
    });
    // SAFETY: AVMediaTypeAudio is valid; the block is called once, on an
    // arbitrary queue, and only sends on a channel.
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler) };
    match rx.recv() {
        Ok(true) => Access::Allowed,
        _ => microphone_access(),
    }
}

pub fn open_privacy_settings(pane: PrivacyPane) {
    let url = NSURL::URLWithString(&NSString::from_str(pane.url()));
    if let Some(url) = url {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_what_the_screen_shows() {
        assert_eq!(access(AVAuthorizationStatus::Authorized), Access::Allowed);
        assert_eq!(
            access(AVAuthorizationStatus::NotDetermined),
            Access::NotAsked
        );
        assert_eq!(access(AVAuthorizationStatus::Denied), Access::Denied);
        assert_eq!(access(AVAuthorizationStatus::Restricted), Access::Denied);
    }

    #[test]
    fn reading_the_microphone_permission_does_not_ask() {
        // Whatever this Mac says, reading it must not block or prompt.
        let _ = microphone_access();
    }

    #[test]
    fn settings_links_stay_on_this_mac() {
        for pane in [PrivacyPane::Microphone, PrivacyPane::ComputerAudio] {
            assert!(pane.url().starts_with("x-apple.systempreferences:"));
        }
    }
}
