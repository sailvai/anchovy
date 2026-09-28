//! Core Audio calls for recording, through `objc2-core-audio` (plan step 4a).
//! Kept thin: list input devices, make a system-audio tap that leaves out
//! Anchovy's own process, put it and the microphone in one private aggregate
//! device, and run an IO proc that hands each cycle to `core::Feed`. Every
//! decision lives in `core.rs`.

use std::ffi::{c_void, CStr};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AllocAnyThread;
use objc2_core_audio::*;
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};
use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use super::core::{
    listable, pick_microphone, rings, Capture, Feed, InputDevice, IoTeardown, RecordingError,
    Started, StreamLayout, MAX_STREAMS, OWN_DEVICE_UID_PREFIX,
};
use crate::notes::folder::StartTime;

type Result<T> = std::result::Result<T, RecordingError>;

fn check(status: i32, what: &str) -> Result<()> {
    if status == 0 {
        return Ok(());
    }
    let bytes = status.to_be_bytes();
    let fourcc = if bytes.iter().all(|b| b.is_ascii_graphic()) {
        format!(" '{}'", String::from_utf8_lossy(&bytes))
    } else {
        String::new()
    };
    Err(RecordingError::Device(format!(
        "{what} failed (OSStatus {status}{fourcc})"
    )))
}

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn get_qualified<T: Copy, Q>(
    object: u32,
    selector: u32,
    scope: u32,
    qualifier: Option<&Q>,
    initial: T,
) -> Result<T> {
    let mut addr = address(selector, scope);
    let mut value = initial;
    let mut size = size_of::<T>() as u32;
    let (qualifier_size, qualifier_ptr) = match qualifier {
        Some(q) => (size_of::<Q>() as u32, q as *const Q as *const c_void),
        None => (0, std::ptr::null()),
    };
    // SAFETY: `value` is a valid, writable T of `size` bytes, and the
    // qualifier pointer (if any) points to `qualifier_size` readable bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            qualifier_size,
            qualifier_ptr,
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    check(status, "Reading an audio property")?;
    Ok(value)
}

fn get<T: Copy>(object: u32, selector: u32, scope: u32, initial: T) -> Result<T> {
    get_qualified::<T, ()>(object, selector, scope, None, initial)
}

fn get_string(object: u32, selector: u32) -> Result<String> {
    let raw: *const CFString = get(
        object,
        selector,
        kAudioObjectPropertyScopeGlobal,
        std::ptr::null(),
    )?;
    let string = NonNull::new(raw as *mut CFString)
        .ok_or_else(|| RecordingError::Device("an audio device has no name".into()))?;
    // SAFETY: Core Audio returns a +1 retained CFString for these properties.
    Ok(unsafe { CFRetained::from_raw(string) }.to_string())
}

/// A variable-size property, in u64 storage so an AudioBufferList read into
/// it is aligned.
fn get_bytes(object: u32, selector: u32, scope: u32) -> Result<Vec<u64>> {
    let mut addr = address(selector, scope);
    let mut size = 0u32;
    // SAFETY: plain out-parameter call.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    check(status, "Reading an audio property size")?;
    let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
    // SAFETY: `buf` holds at least `size` writable bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(buf.as_mut_ptr()).unwrap().cast(),
        )
    };
    check(status, "Reading an audio property")?;
    Ok(buf)
}

/// Channel count of each input stream, in the order the IO proc gets them.
fn input_streams(device: u32) -> Result<Vec<usize>> {
    let buf = get_bytes(
        device,
        kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeInput,
    )?;
    let list = buf.as_ptr() as *const AudioBufferList;
    // SAFETY: Core Audio filled `buf` with an AudioBufferList whose
    // mNumberBuffers entries all fit in it.
    unsafe {
        let count = (*list).mNumberBuffers as usize;
        let first = (*list).mBuffers.as_ptr();
        Ok((0..count)
            .map(|i| (*first.add(i)).mNumberChannels as usize)
            .collect())
    }
}

struct Device {
    id: u32,
    info: InputDevice,
    streams: Vec<usize>,
}

fn devices() -> Result<Vec<Device>> {
    let buf = get_bytes(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )?;
    // SAFETY: the property is an array of AudioObjectIDs (u32).
    let ids: &[u32] =
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u32, buf.len() * 2) };
    let mut out = Vec::new();
    for &id in ids.iter().filter(|&&id| id != kAudioObjectUnknown) {
        let Ok(streams) = input_streams(id) else {
            continue;
        };
        if streams.iter().sum::<usize>() == 0 {
            continue;
        }
        out.push(Device {
            id,
            info: InputDevice {
                uid: get_string(id, kAudioDevicePropertyDeviceUID)?,
                name: get_string(id, kAudioObjectPropertyName)?,
            },
            streams,
        });
    }
    Ok(out)
}

/// Input devices by their system names, without Anchovy's own.
pub fn input_devices() -> Result<Vec<InputDevice>> {
    Ok(listable(devices()?.into_iter().map(|d| d.info).collect()))
}

/// UID of the system default input, if there is one.
pub fn default_input_uid() -> Option<String> {
    let id: u32 = get(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDefaultInputDevice,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectUnknown,
    )
    .ok()?;
    if id == kAudioObjectUnknown {
        return None;
    }
    get_string(id, kAudioDevicePropertyDeviceUID).ok()
}

/// Anchovy's own Core Audio process object, so the tap can leave it out.
/// `None` if Core Audio does not list this process.
pub fn own_process_object() -> Option<u32> {
    let pid: libc::pid_t = std::process::id() as libc::pid_t;
    let id: u32 = get_qualified(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyTranslatePIDToProcessObject,
        kAudioObjectPropertyScopeGlobal,
        Some(&pid),
        kAudioObjectUnknown,
    )
    .ok()?;
    (id != kAudioObjectUnknown).then_some(id)
}

/// A private, unmuted stereo tap of every process's output except
/// `excluded`.
fn create_tap(excluded: &[u32]) -> Result<(u32, String)> {
    // SAFETY: CATapDescription is created and configured on this thread
    // before Core Audio sees it.
    let description = unsafe {
        let numbers: Vec<Retained<NSNumber>> =
            excluded.iter().map(|&id| NSNumber::new_u32(id)).collect();
        let excluded = NSArray::from_retained_slice(&numbers);
        let d = CATapDescription::initStereoGlobalTapButExcludeProcesses(
            CATapDescription::alloc(),
            &excluded,
        );
        d.setName(&NSString::from_str("Anchovy"));
        d.setPrivate(true);
        d.setMuteBehavior(CATapMuteBehavior::Unmuted);
        d
    };
    // SAFETY: reading the UUID the description generated.
    let uid = unsafe { description.UUID().UUIDString() }.to_string();
    let mut id = kAudioObjectUnknown;
    // SAFETY: `description` is a valid CATapDescription; `id` is written.
    check(
        unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut id) },
        "Creating the computer audio tap",
    )?;
    Ok((id, uid))
}

fn destroy_tap(id: u32) -> Result<()> {
    // SAFETY: `id` is a tap this process created.
    check(
        unsafe { AudioHardwareDestroyProcessTap(id) },
        "Removing the computer audio tap",
    )
}

fn key(k: &CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap())
}

fn dict(pairs: &[(&CStr, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| key(k)).collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&key_refs, &values)
}

/// A private aggregate device: the microphone as main (clock) sub-device, the
/// tap in its tap list with drift compensation.
///
/// Tap auto-start stays off. With it on, the device does not run while no
/// process is playing sound, so a recording started in a quiet room got no
/// audio until something played (found by the verify:device loopback).
fn create_aggregate(mic_uid: &str, tap_uid: &str) -> Result<u32> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let mic = NSString::from_str(mic_uid);
    let tap = NSString::from_str(tap_uid);
    let yes = NSNumber::new_bool(true);
    let sub_device = dict(&[(kAudioSubDeviceUIDKey, &mic)]);
    let sub_tap = dict(&[
        (kAudioSubTapUIDKey, &tap),
        (kAudioSubTapDriftCompensationKey, &yes),
    ]);
    let sub_devices = NSArray::from_retained_slice(&[sub_device]);
    let taps = NSArray::from_retained_slice(&[sub_tap]);
    let uid = NSString::from_str(&format!(
        "{OWN_DEVICE_UID_PREFIX}recording.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let name = NSString::from_str("Anchovy");
    let description = dict(&[
        (kAudioAggregateDeviceUIDKey, &uid),
        (kAudioAggregateDeviceNameKey, &name),
        (kAudioAggregateDeviceMainSubDeviceKey, &mic),
        (kAudioAggregateDeviceIsPrivateKey, &yes),
        (
            kAudioAggregateDeviceIsStackedKey,
            &NSNumber::new_bool(false),
        ),
        (
            kAudioAggregateDeviceTapAutoStartKey,
            &NSNumber::new_bool(false),
        ),
        (kAudioAggregateDeviceSubDeviceListKey, &sub_devices),
        (kAudioAggregateDeviceTapListKey, &taps),
    ]);
    // SAFETY: NSDictionary is toll-free bridged to CFDictionary.
    let cf = unsafe { &*(Retained::as_ptr(&description) as *const CFDictionary) };
    let mut id = kAudioObjectUnknown;
    // SAFETY: `cf` is a valid dictionary; `id` is written.
    check(
        unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut id)) },
        "Creating the recording device",
    )?;
    Ok(id)
}

fn destroy_aggregate(id: u32) -> Result<()> {
    // SAFETY: `id` is an aggregate device this process created.
    check(
        unsafe { AudioHardwareDestroyAggregateDevice(id) },
        "Removing the recording device",
    )
}

fn nominal_sample_rate(device: u32) -> Result<f64> {
    get(
        device,
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
        0f64,
    )
}

/// Runs on Core Audio's real-time thread: no locks, no allocation.
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: `context` is the Feed registered with this proc. Core Audio
    // calls one IO proc serially, so this is the only reference to it.
    let feed = unsafe { &mut *(context as *mut Feed) };
    let list = input.as_ptr();
    let mut buffers: [&[f32]; MAX_STREAMS] = [&[]; MAX_STREAMS];
    // SAFETY: the input list has mNumberBuffers valid AudioBuffers of 32-bit
    // float samples, valid for the duration of this call.
    let count = unsafe {
        let count = ((*list).mNumberBuffers as usize).min(MAX_STREAMS);
        let first = (*list).mBuffers.as_ptr();
        for (i, slot) in buffers.iter_mut().enumerate().take(count) {
            let b = &*first.add(i);
            if !b.mData.is_null() {
                *slot = std::slice::from_raw_parts(
                    b.mData as *const f32,
                    b.mDataByteSize as usize / size_of::<f32>(),
                );
            }
        }
        count
    };
    feed.deliver(&buffers[..count]);
    0
}

/// A registered, running IO proc and the Feed it writes into.
struct Io {
    device: u32,
    proc_id: AudioDeviceIOProcID,
    feed: *mut Feed,
}

fn start_io(device: u32, feed: Feed) -> Result<Io> {
    let feed = Box::into_raw(Box::new(feed));
    let mut proc_id: AudioDeviceIOProcID = None;
    // SAFETY: `feed` stays alive until the proc is stopped and destroyed.
    let created = check(
        unsafe {
            AudioDeviceCreateIOProcID(
                device,
                Some(io_proc),
                feed as *mut c_void,
                NonNull::from(&mut proc_id),
            )
        },
        "Registering for audio",
    );
    if let Err(err) = created {
        // SAFETY: the proc was never registered, so nothing else has `feed`.
        drop(unsafe { Box::from_raw(feed) });
        return Err(err);
    }
    let io = Io {
        device,
        proc_id,
        feed,
    };
    // SAFETY: the proc was just registered on `device`.
    if let Err(err) = check(
        unsafe { AudioDeviceStart(device, proc_id) },
        "Starting audio",
    ) {
        let _ = stop_io(io, false);
        return Err(err);
    }
    Ok(io)
}

fn stop_io(io: Io, started: bool) -> Result<()> {
    // SAFETY: `io` holds a proc registered on `io.device`.
    let stopped = if started {
        check(
            unsafe { AudioDeviceStop(io.device, io.proc_id) },
            "Stopping audio",
        )
    } else {
        Ok(())
    };
    let destroyed = check(
        unsafe { AudioDeviceDestroyIOProcID(io.device, io.proc_id) },
        "Unregistering from audio",
    );
    let teardown = IoTeardown {
        started,
        stopped: stopped.is_ok(),
        destroyed: destroyed.is_ok(),
    };
    if teardown.may_free_context() {
        // SAFETY: the proc can no longer run, so nothing else uses `feed`.
        drop(unsafe { Box::from_raw(io.feed) });
    }
    // Otherwise the Feed is leaked on purpose: Core Audio may still call the
    // proc with it.
    stopped.and(destroyed)
}

struct MacCapture {
    io: Io,
    aggregate: Option<u32>,
    tap: Option<u32>,
}

// SAFETY: the raw Feed pointer is only touched by Core Audio's IO thread
// while the proc runs, and by `stop` after it has stopped.
unsafe impl Send for MacCapture {}

impl Capture for MacCapture {
    fn stop(self: Box<Self>) -> Result<()> {
        let io = stop_io(self.io, true);
        let aggregate = self.aggregate.map_or(Ok(()), destroy_aggregate);
        let tap = self.tap.map_or(Ok(()), destroy_tap);
        io.and(aggregate).and(tap)
    }
}

/// Microphone plus computer audio through one aggregate device.
fn start_with_computer_audio(mic: &Device) -> Result<(MacCapture, f64, super::core::Drain)> {
    let excluded: Vec<u32> = own_process_object().into_iter().collect();
    let (tap, tap_uid) = create_tap(&excluded)?;
    let with_tap = (|| {
        let aggregate = create_aggregate(&mic.info.uid, &tap_uid)?;
        let running = (|| {
            let layout = StreamLayout::split(&input_streams(aggregate)?, mic.streams.len())?;
            if !layout.has_computer_audio() {
                return Err(RecordingError::Device(
                    "the recording device has no computer audio stream".into(),
                ));
            }
            let rate = nominal_sample_rate(aggregate)?;
            let (feed, drain) = rings(rate, layout);
            Ok((start_io(aggregate, feed)?, rate, drain))
        })();
        match running {
            Ok((io, rate, drain)) => Ok((
                MacCapture {
                    io,
                    aggregate: Some(aggregate),
                    tap: Some(tap),
                },
                rate,
                drain,
            )),
            Err(err) => {
                let _ = destroy_aggregate(aggregate);
                Err(err)
            }
        }
    })();
    if with_tap.is_err() {
        let _ = destroy_tap(tap);
    }
    with_tap
}

fn start_microphone_only(mic: &Device) -> Result<(MacCapture, f64, super::core::Drain)> {
    let layout = StreamLayout::split(&mic.streams, mic.streams.len())?;
    let rate = nominal_sample_rate(mic.id)?;
    let (feed, drain) = rings(rate, layout);
    let io = start_io(mic.id, feed)?;
    Ok((
        MacCapture {
            io,
            aggregate: None,
            tap: None,
        },
        rate,
        drain,
    ))
}

/// Starts capturing from the chosen microphone (or the default one) plus
/// computer audio. If computer audio cannot be captured, records the
/// microphone only; the returned `Drain` says which.
pub fn start(wanted_uid: Option<&str>) -> Result<Started> {
    let devices = devices()?;
    let infos = listable(devices.iter().map(|d| d.info.clone()).collect());
    let default = default_input_uid();
    let chosen = pick_microphone(&infos, wanted_uid, default.as_deref())
        .ok_or(RecordingError::NoMicrophone)?;
    let mic = devices
        .iter()
        .find(|d| d.info.uid == chosen.uid)
        .ok_or(RecordingError::NoMicrophone)?;
    let (capture, sample_rate, drain) = start_with_computer_audio(mic).or_else(|err| {
        eprintln!("Recording the microphone only. {err}");
        start_microphone_only(mic)
    })?;
    Ok(Started {
        capture: Box::new(capture),
        sample_rate,
        drain,
        microphone: mic.info.name.clone(),
    })
}

/// The local wall-clock time now.
pub fn local_now() -> StartTime {
    let now: libc::time_t = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: `tm` is plain data; localtime_r fills it from `now`.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    };
    StartTime {
        year: (tm.tm_year + 1900) as u16,
        month: (tm.tm_mon + 1) as u8,
        day: tm.tm_mday as u8,
        hour: tm.tm_hour as u8,
        minute: tm.tm_min as u8,
        second: tm.tm_sec as u8,
    }
}

/// `~/Documents/Anchovy`, the default notes folder, until the first-launch
/// screens (plan step 3) let the user choose one. Inside the app sandbox
/// `HOME` is the app's container.
pub fn default_notes_dir() -> std::io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set"))?;
    let dir = PathBuf::from(home).join("Documents/Anchovy");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_now_matches_the_date_command() {
        let output = std::process::Command::new("date")
            .arg("+%Y-%m-%d-%H%M")
            .output()
            .unwrap();
        let expected = String::from_utf8(output.stdout).unwrap();
        let now = local_now().folder_name();
        // Allow for the minute turning over between the two reads.
        assert!(
            expected.trim() == now || local_now().second < 2,
            "{now} vs {expected}"
        );
    }

    #[test]
    fn listing_input_devices_does_not_fail() {
        // CI machines may have no input devices; the call must still work.
        let devices = input_devices().unwrap();
        assert!(devices
            .iter()
            .all(|d| !d.uid.starts_with(OWN_DEVICE_UID_PREFIX) && !d.name.is_empty()));
    }
}
