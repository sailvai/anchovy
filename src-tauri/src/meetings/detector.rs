//! Whether a meeting has started, from snapshots of the processes that use
//! the microphone. No clock and no system calls: the caller passes each
//! snapshot with the time it was taken, so tests use fake snapshots and a
//! fake clock.
//!
//! Zoom and Teams count as a meeting. A browser using the microphone may be
//! a call, so its prompt says so. Window titles are never read.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long a process must keep using the microphone, or keep not using it,
/// before the change counts. A short blip never prompts, and a short gap
/// never ends a meeting.
pub const SETTLE: Duration = Duration::from_secs(5);

/// One process in the audio system's process list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: i32,
    /// Empty when the audio system does not know it.
    pub bundle_id: String,
    pub using_microphone: bool,
}

/// An app whose microphone use may be a meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum App {
    Zoom,
    Teams,
    Chrome,
    Safari,
    Edge,
    Arc,
    Firefox,
}

impl App {
    const ALL: [App; 7] = [
        App::Zoom,
        App::Teams,
        App::Chrome,
        App::Safari,
        App::Edge,
        App::Arc,
        App::Firefox,
    ];

    pub fn name(self) -> &'static str {
        match self {
            App::Zoom => "Zoom",
            App::Teams => "Teams",
            App::Chrome => "Chrome",
            App::Safari => "Safari",
            App::Edge => "Edge",
            App::Arc => "Arc",
            App::Firefox => "Firefox",
        }
    }

    /// Browsers may be in a call or may be doing something else.
    pub fn is_browser(self) -> bool {
        !matches!(self, App::Zoom | App::Teams)
    }

    /// The app's bundle IDs. Helper processes, such as
    /// `com.google.Chrome.helper`, use the microphone for browsers, so an ID
    /// also matches IDs that continue it after a dot.
    fn bundle_ids(self) -> &'static [&'static str] {
        match self {
            App::Zoom => &["us.zoom.xos"],
            App::Teams => &["com.microsoft.teams2"],
            App::Chrome => &["com.google.Chrome"],
            // Safari's media runs in WebKit's GPU process.
            App::Safari => &["com.apple.Safari", "com.apple.WebKit.GPU"],
            App::Edge => &["com.microsoft.edgemac"],
            App::Arc => &["company.thebrowser.Browser"],
            App::Firefox => &["org.mozilla.firefox", "org.mozilla.plugincontainer"],
        }
    }

    pub fn from_bundle_id(bundle_id: &str) -> Option<App> {
        App::ALL
            .into_iter()
            .find(|app| app.bundle_ids().iter().any(|id| same_app(bundle_id, id)))
    }

    /// The first line of the prompt, in the banner and the notification.
    pub fn headline(self) -> String {
        if self.is_browser() {
            format!("A call may have started in {}.", self.name())
        } else {
            format!("{} meeting started.", self.name())
        }
    }
}

/// The second line of the prompt.
pub const PROMPT_BODY: &str = "Record it? Anchovy records only if you choose Record.";

/// `bundle_id` is `app_id` or one of its helpers (`app_id.something`).
fn same_app(bundle_id: &str, app_id: &str) -> bool {
    bundle_id
        .strip_prefix(app_id)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// A question that waits for Record or Not now. `id` is new for every
/// prompt, so an answer to one that has gone away is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prompt {
    pub id: u64,
    pub app: App,
    pub headline: String,
    pub body: &'static str,
}

/// One app's microphone use over time.
#[derive(Debug)]
struct Track {
    /// The use that has lasted `SETTLE`.
    active: bool,
    /// When the latest snapshot started to disagree with `active`.
    changing_since: Option<Instant>,
    /// When `active` last became true; earlier meetings are asked first.
    started: Instant,
    /// Asked already, or not to be asked, for this meeting: prompted,
    /// answered, or started while Anchovy could not ask.
    settled: bool,
}

/// Turns process snapshots into at most one prompt at a time.
#[derive(Debug)]
pub struct Detector {
    own_pid: i32,
    own_bundle_id: String,
    apps: BTreeMap<App, Track>,
    prompt: Option<Prompt>,
    next_id: u64,
}

impl Detector {
    /// `own_pid` and `own_bundle_id` are Anchovy's, which is never a meeting.
    pub fn new(own_pid: i32, own_bundle_id: &str) -> Self {
        Detector {
            own_pid,
            own_bundle_id: own_bundle_id.to_owned(),
            apps: BTreeMap::new(),
            prompt: None,
            next_id: 1,
        }
    }

    /// Takes the processes seen at `now` and returns the prompt to show, if
    /// any. `may_ask` is false while recording, and when Record could not
    /// start; a meeting that starts then is not asked about later.
    pub fn observe(
        &mut self,
        processes: &[Process],
        may_ask: bool,
        now: Instant,
    ) -> Option<&Prompt> {
        let using: Vec<App> = processes
            .iter()
            .filter(|p| p.using_microphone && !self.is_own(p))
            .filter_map(|p| App::from_bundle_id(&p.bundle_id))
            .collect();
        for app in &using {
            self.apps.entry(*app).or_insert(Track {
                active: false,
                changing_since: None,
                started: now,
                settled: false,
            });
        }

        let mut ended = Vec::new();
        for (app, track) in &mut self.apps {
            let seen = using.contains(app);
            if seen == track.active {
                track.changing_since = None;
                continue;
            }
            let since = *track.changing_since.get_or_insert(now);
            if now.duration_since(since) < SETTLE {
                continue;
            }
            track.active = seen;
            track.changing_since = None;
            if seen {
                track.started = now;
                track.settled = false;
            } else {
                ended.push(*app);
            }
        }
        for app in ended {
            self.apps.remove(&app);
            if self.prompt.as_ref().is_some_and(|p| p.app == app) {
                self.prompt = None;
            }
        }
        // Apps seen once that never settled are forgotten once they stop.
        self.apps
            .retain(|_, track| track.active || track.changing_since.is_some());

        if !may_ask {
            self.prompt = None;
            for track in self.apps.values_mut().filter(|t| t.active) {
                track.settled = true;
            }
            return None;
        }
        if self.prompt.is_none() {
            let next = self
                .apps
                .iter_mut()
                .filter(|(_, t)| t.active && !t.settled)
                .min_by_key(|(app, t)| (t.started, **app));
            if let Some((app, track)) = next {
                track.settled = true;
                self.prompt = Some(Prompt {
                    id: self.next_id,
                    app: *app,
                    headline: app.headline(),
                    body: PROMPT_BODY,
                });
                self.next_id += 1;
            }
        }
        self.prompt.as_ref()
    }

    fn is_own(&self, process: &Process) -> bool {
        process.pid == self.own_pid || same_app(&process.bundle_id, &self.own_bundle_id)
    }

    /// Record or Not now on prompt `id`. Either way the prompt goes away and
    /// this meeting is not asked about again. Returns false if `id` is no
    /// longer the prompt, so a late answer does nothing.
    pub fn answer(&mut self, id: u64) -> bool {
        if self.prompt.as_ref().is_some_and(|p| p.id == id) {
            self.prompt = None;
            true
        } else {
            false
        }
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN_PID: i32 = 500;
    const OWN_ID: &str = "com.sailvai.anchovy";

    fn process(pid: i32, bundle_id: &str, using_microphone: bool) -> Process {
        Process {
            pid,
            bundle_id: bundle_id.into(),
            using_microphone,
        }
    }

    fn zoom(using: bool) -> Vec<Process> {
        vec![
            process(10, "com.apple.controlcenter", false),
            process(20, "us.zoom.xos", using),
        ]
    }

    /// A detector and a fake clock that starts at 0 s.
    struct Run {
        detector: Detector,
        start: Instant,
    }

    impl Run {
        fn new() -> Self {
            Run {
                detector: Detector::new(OWN_PID, OWN_ID),
                start: Instant::now(),
            }
        }

        /// Feeds the same snapshot once a second from `from` to `to`
        /// seconds, both included, and returns the last prompt.
        fn feed(
            &mut self,
            processes: &[Process],
            may_ask: bool,
            from: u64,
            to: u64,
        ) -> Option<Prompt> {
            let mut last = None;
            for second in from..=to {
                let now = self.start + Duration::from_secs(second);
                last = self.detector.observe(processes, may_ask, now).cloned();
            }
            last
        }
    }

    #[test]
    fn zoom_using_the_microphone_for_five_seconds_is_a_meeting() {
        let mut run = Run::new();
        assert_eq!(run.feed(&zoom(true), true, 0, 4), None);

        let prompt = run.feed(&zoom(true), true, 5, 5).expect("a prompt");

        assert_eq!(prompt.app, App::Zoom);
        assert_eq!(prompt.headline, "Zoom meeting started.");
        assert_eq!(
            prompt.body,
            "Record it? Anchovy records only if you choose Record."
        );
    }

    #[test]
    fn teams_is_a_meeting_too() {
        let mut run = Run::new();
        let teams = [process(30, "com.microsoft.teams2", true)];
        let prompt = run.feed(&teams, true, 0, 5).expect("a prompt");
        assert_eq!(prompt.app, App::Teams);
        assert_eq!(prompt.headline, "Teams meeting started.");
    }

    #[test]
    fn a_browser_may_be_a_call_and_is_named() {
        for (bundle_id, name) in [
            ("com.google.Chrome.helper", "Chrome"),
            ("com.google.Chrome", "Chrome"),
            ("com.apple.WebKit.GPU", "Safari"),
            ("com.microsoft.edgemac.helper", "Edge"),
            ("company.thebrowser.Browser.helper", "Arc"),
            ("org.mozilla.firefox", "Firefox"),
        ] {
            let mut run = Run::new();
            let prompt = run
                .feed(&[process(40, bundle_id, true)], true, 0, 5)
                .unwrap_or_else(|| panic!("{bundle_id}"));
            assert_eq!(
                prompt.headline,
                format!("A call may have started in {name}.")
            );
        }
    }

    #[test]
    fn other_apps_and_look_alike_ids_are_not_meetings() {
        let mut run = Run::new();
        let others = [
            process(50, "com.apple.VoiceMemos", true),
            process(51, "us.zoom.xosx", true),
            process(52, "", true),
        ];
        assert_eq!(run.feed(&others, true, 0, 30), None);
    }

    #[test]
    fn a_short_blip_does_not_count() {
        let mut run = Run::new();
        run.feed(&zoom(true), true, 0, 3);
        assert_eq!(run.feed(&zoom(false), true, 4, 30), None);

        // Nor do blips that keep restarting the wait.
        for start in (40..80).step_by(6) {
            assert_eq!(run.feed(&zoom(true), true, start, start + 3), None);
            assert_eq!(run.feed(&zoom(false), true, start + 4, start + 5), None);
        }
    }

    #[test]
    fn anchovy_itself_is_excluded() {
        let mut run = Run::new();
        // Anchovy's own process, even if it had a meeting app's ID, and its
        // helpers.
        let own = [
            process(OWN_PID, "us.zoom.xos", true),
            process(501, OWN_ID, true),
            process(502, "com.sailvai.anchovy.helper", true),
        ];
        assert_eq!(run.feed(&own, true, 0, 30), None);
    }

    #[test]
    fn no_prompt_while_recording() {
        let mut run = Run::new();
        assert_eq!(run.feed(&zoom(true), false, 0, 30), None);
    }

    #[test]
    fn a_meeting_that_started_while_recording_is_not_asked_about_after() {
        let mut run = Run::new();
        run.feed(&zoom(true), false, 0, 30);
        assert_eq!(run.feed(&zoom(true), true, 31, 60), None);
    }

    #[test]
    fn starting_to_record_takes_the_prompt_away() {
        let mut run = Run::new();
        assert!(run.feed(&zoom(true), true, 0, 5).is_some());
        assert_eq!(run.feed(&zoom(true), false, 6, 6), None);
        assert_eq!(run.feed(&zoom(true), true, 7, 60), None);
    }

    #[test]
    fn not_now_stops_asking_for_this_meeting() {
        let mut run = Run::new();
        let prompt = run.feed(&zoom(true), true, 0, 5).unwrap();

        assert!(run.detector.answer(prompt.id));

        assert_eq!(run.detector.prompt(), None);
        assert_eq!(run.feed(&zoom(true), true, 6, 600), None);
    }

    #[test]
    fn not_now_is_for_that_app_only() {
        let mut run = Run::new();
        let prompt = run.feed(&zoom(true), true, 0, 5).unwrap();
        run.detector.answer(prompt.id);

        let mut both = zoom(true);
        both.push(process(30, "com.microsoft.teams2", true));
        assert_eq!(run.feed(&both, true, 6, 10), None);
        assert_eq!(run.feed(&both, true, 11, 11).unwrap().app, App::Teams);
    }

    #[test]
    fn the_next_meeting_is_asked_about_again() {
        let mut run = Run::new();
        let first = run.feed(&zoom(true), true, 0, 5).unwrap();
        run.detector.answer(first.id);
        run.feed(&zoom(false), true, 6, 20);

        let second = run.feed(&zoom(true), true, 21, 26).expect("a new prompt");
        assert_ne!(second.id, first.id);
    }

    #[test]
    fn a_late_answer_does_nothing() {
        let mut run = Run::new();
        let first = run.feed(&zoom(true), true, 0, 5).unwrap();
        run.feed(&zoom(false), true, 6, 20);
        let second = run.feed(&zoom(true), true, 21, 26).unwrap();

        assert!(!run.detector.answer(first.id));
        assert_eq!(run.detector.prompt(), Some(&second));
    }

    #[test]
    fn the_prompt_goes_away_when_the_meeting_ends() {
        let mut run = Run::new();
        assert!(run.feed(&zoom(true), true, 0, 5).is_some());

        // A short gap in microphone use is not the end.
        assert!(run.feed(&zoom(false), true, 6, 9).is_some());
        assert!(run.feed(&zoom(true), true, 10, 10).is_some());

        assert!(run.feed(&zoom(false), true, 11, 15).is_some());
        assert_eq!(run.feed(&zoom(false), true, 16, 16), None);
        // Zoom quitting removes it from the list altogether.
        assert_eq!(run.feed(&[], true, 17, 60), None);
    }

    #[test]
    fn a_meeting_that_disappears_from_the_list_ends_too() {
        let mut run = Run::new();
        assert!(run.feed(&zoom(true), true, 0, 5).is_some());
        assert!(run.feed(&[], true, 6, 10).is_some());
        assert_eq!(run.feed(&[], true, 11, 11), None);
    }

    #[test]
    fn one_prompt_at_a_time_and_the_next_follows() {
        let mut run = Run::new();
        let mut both = zoom(true);
        run.feed(&both, true, 0, 2);
        both.push(process(30, "com.microsoft.teams2", true));

        let first = run.feed(&both, true, 3, 10).unwrap();
        assert_eq!(first.app, App::Zoom);

        // Zoom ends; Teams is still going and has not been asked about.
        let teams_only = [process(30, "com.microsoft.teams2", true)];
        assert_eq!(run.feed(&teams_only, true, 11, 15).unwrap().app, App::Zoom);
        assert_eq!(run.feed(&teams_only, true, 16, 16).unwrap().app, App::Teams);
    }
}
