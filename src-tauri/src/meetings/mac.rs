//! The thin macOS layer for the meeting prompt: which processes the audio
//! system lists, and a notification with Record and Not now.
//!
//! Only public APIs: Core Audio's process objects (macOS 14.2 and later) and
//! UserNotifications. No window titles, so no screen recording permission.

use std::sync::OnceLock;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_core_audio::{
    kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject, kAudioObjectUnknown, kAudioProcessPropertyBundleID,
    kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID,
};
use objc2_foundation::{NSArray, NSBundle, NSError, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

use super::detector::{Process, Prompt};
use crate::recording::mac::{get, get_bytes, get_string};

/// Every process the audio system lists, with whether it is using an input
/// right now. Empty if the list cannot be read.
pub fn processes() -> Vec<Process> {
    let Ok(raw) = get_bytes(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyProcessObjectList,
        kAudioObjectPropertyScopeGlobal,
    ) else {
        return Vec::new();
    };
    // The list is u32 object IDs; the u64 storage may end in padding, which
    // reads as kAudioObjectUnknown.
    raw.iter()
        .flat_map(|pair| [*pair as u32, (*pair >> 32) as u32])
        .filter(|&id| id != kAudioObjectUnknown)
        .filter_map(|id| {
            let global = kAudioObjectPropertyScopeGlobal;
            Some(Process {
                pid: get(id, kAudioProcessPropertyPID, global, -1i32).ok()?,
                bundle_id: get_string(id, kAudioProcessPropertyBundleID).unwrap_or_default(),
                using_microphone: get(id, kAudioProcessPropertyIsRunningInput, global, 0u32)
                    .ok()?
                    != 0,
            })
        })
        .collect()
}

/// Anchovy's bundle ID, or `None` when it runs outside an app bundle, as in
/// development, where macOS has no notification center for it.
pub fn own_bundle_id() -> Option<String> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        let bundle = NSBundle::mainBundle();
        let bundled = bundle.bundlePath().to_string().ends_with(".app");
        bundle
            .bundleIdentifier()
            .filter(|_| bundled)
            .map(|id| id.to_string())
    })
    .clone()
}

/// What the user did with a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Record,
    NotNow,
    /// Clicked the notification itself.
    Open,
}

const CATEGORY: &str = "meeting-prompt";
const RECORD_ACTION: &str = "record";
const NOT_NOW_ACTION: &str = "not-now";
const REQUEST_PREFIX: &str = "meeting-prompt-";

fn request_id(prompt_id: u64) -> String {
    format!("{REQUEST_PREFIX}{prompt_id}")
}

fn prompt_id(request_id: &str) -> Option<u64> {
    request_id.strip_prefix(REQUEST_PREFIX)?.parse().ok()
}

struct Ivars {
    on_choice: Box<dyn Fn(u64, Choice) + Send + Sync>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Delegate does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "AnchovyMeetingNotifications"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Show the prompt even while Anchovy is the frontmost app.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let action = response.actionIdentifier().to_string();
            let request = response.notification().request().identifier().to_string();
            // SAFETY: a constant NSString from UserNotifications.
            let default_action = unsafe { UNNotificationDefaultActionIdentifier }.to_string();
            let choice = match action.as_str() {
                RECORD_ACTION => Some(Choice::Record),
                NOT_NOW_ACTION => Some(Choice::NotNow),
                a if a == default_action => Some(Choice::Open),
                // Dismissed: the banner in the window still asks.
                _ => None,
            };
            if let (Some(id), Some(choice)) = (prompt_id(&request), choice) {
                (self.ivars().on_choice)(id, choice);
            }
            completion.call(());
        }
    }
);

fn center() -> Option<Retained<UNUserNotificationCenter>> {
    own_bundle_id().map(|_| UNUserNotificationCenter::currentNotificationCenter())
}

/// Registers the Record and Not now buttons and the handler for them. Does
/// nothing outside an app bundle.
pub fn install(on_choice: impl Fn(u64, Choice) + Send + Sync + 'static) {
    let Some(center) = center() else { return };
    let actions = [
        UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(RECORD_ACTION),
            &NSString::from_str("Record"),
            UNNotificationActionOptions::empty(),
        ),
        UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(NOT_NOW_ACTION),
            &NSString::from_str("Not now"),
            UNNotificationActionOptions::empty(),
        ),
    ];
    let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
        &NSString::from_str(CATEGORY),
        &NSArray::from_retained_slice(&actions),
        &NSArray::new(),
        UNNotificationCategoryOptions::empty(),
    );
    center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));

    let delegate = Delegate::alloc().set_ivars(Ivars {
        on_choice: Box::new(on_choice),
    });
    // SAFETY: NSObject's init on a freshly allocated object.
    let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center holds its delegate weakly; this one lives as long as the app.
    std::mem::forget(delegate);
}

/// Shows the prompt as a notification. macOS asks for permission the first
/// time; if notifications are not allowed, the banner in the window is the
/// only prompt.
pub fn post(prompt: &Prompt) {
    let Some(center) = center() else { return };
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&prompt.headline));
    content.setBody(&NSString::from_str(prompt.body));
    content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&request_id(prompt.id)),
        &content,
        None,
    );
    let add = center.clone();
    let then_add = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if granted.as_bool() {
            add.addNotificationRequest_withCompletionHandler(&request, None);
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert,
        &then_add,
    );
}

/// Takes the notification for prompt `id` away, shown or not yet shown.
pub fn remove(id: u64) {
    let Some(center) = center() else { return };
    let ids = NSArray::from_retained_slice(&[NSString::from_str(&request_id(id))]);
    center.removePendingNotificationRequestsWithIdentifiers(&ids);
    center.removeDeliveredNotificationsWithIdentifiers(&ids);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notification_carries_its_prompt_id() {
        assert_eq!(prompt_id(&request_id(42)), Some(42));
        assert_eq!(prompt_id("meeting-prompt-x"), None);
        assert_eq!(prompt_id("other-42"), None);
    }

    #[test]
    fn reading_the_process_list_does_not_fail() {
        assert!(processes().iter().all(|p| p.pid > 0));
    }
}
