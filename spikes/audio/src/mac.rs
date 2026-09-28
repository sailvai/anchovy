//! Core Audio calls for the spike, all through `objc2-core-audio`. Kept thin:
//! find the microphone, make a global system-audio tap, put both into one
//! private aggregate device, run an IO proc, tear everything down.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::Duration;

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::*;
use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use audio_spike::{IoTeardown, Stream, append_cycle, may_free_io_context};

pub type Result<T> = std::result::Result<T, String>;

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
    Err(format!("{what} failed: OSStatus {status}{fourcc}"))
}

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn get<T: Copy>(object: u32, selector: u32, scope: u32, initial: T) -> Result<T> {
    let mut addr = address(selector, scope);
    let mut value = initial;
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    check(status, &format!("get property {selector:#x} of {object}"))?;
    Ok(value)
}

fn get_string(object: u32, selector: u32) -> Result<String> {
    let raw: *const CFString = get(
        object,
        selector,
        kAudioObjectPropertyScopeGlobal,
        std::ptr::null(),
    )?;
    let string = NonNull::new(raw as *mut CFString).ok_or("empty string property")?;
    Ok(unsafe { CFRetained::from_raw(string) }.to_string())
}

fn get_bytes(object: u32, selector: u32, scope: u32) -> Result<Vec<u64>> {
    let mut addr = address(selector, scope);
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    check(status, "get property size")?;
    // u64 storage keeps AudioBufferList aligned.
    let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
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
    check(status, "get property data")?;
    Ok(buf)
}

/// Channel count of each input stream, in buffer order.
pub fn input_streams(device: u32) -> Result<Vec<usize>> {
    let buf = get_bytes(
        device,
        kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeInput,
    )?;
    let list = buf.as_ptr() as *const AudioBufferList;
    let count = unsafe { (*list).mNumberBuffers } as usize;
    let first = unsafe { (*list).mBuffers.as_ptr() };
    Ok((0..count)
        .map(|i| unsafe { (*first.add(i)).mNumberChannels as usize })
        .collect())
}

pub struct Device {
    pub id: u32,
    pub name: String,
    pub uid: String,
    pub input_channels: usize,
}

pub fn input_devices() -> Result<Vec<Device>> {
    let buf = get_bytes(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )?;
    let ids: &[u32] =
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u32, buf.len() * 2) };
    let mut out = Vec::new();
    for &id in ids.iter().filter(|&&id| id != 0) {
        let Ok(streams) = input_streams(id) else {
            continue;
        };
        let input_channels: usize = streams.iter().sum();
        if input_channels == 0 {
            continue;
        }
        out.push(Device {
            id,
            name: get_string(id, kAudioObjectPropertyName)?,
            uid: get_string(id, kAudioDevicePropertyDeviceUID)?,
            input_channels,
        });
    }
    Ok(out)
}

pub fn default_input_device() -> Result<u32> {
    get(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDefaultInputDevice,
        kAudioObjectPropertyScopeGlobal,
        0u32,
    )
}

pub fn nominal_sample_rate(device: u32) -> Result<f64> {
    get(
        device,
        kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal,
        0f64,
    )
}

pub struct Tap {
    pub id: u32,
    pub uid: String,
    pub format: AudioStreamBasicDescription,
}

/// A private, unmuted, stereo tap of every process's output.
pub fn create_global_tap() -> Result<Tap> {
    let description = unsafe {
        let excluded: Retained<NSArray<NSNumber>> = NSArray::new();
        let d = CATapDescription::initStereoGlobalTapButExcludeProcesses(
            CATapDescription::alloc(),
            &excluded,
        );
        d.setName(&NSString::from_str("Anchovy audio spike tap"));
        d.setPrivate(true);
        d.setMuteBehavior(CATapMuteBehavior::Unmuted);
        d
    };
    let uid = unsafe { description.UUID().UUIDString() }.to_string();
    let mut id = 0u32;
    check(
        unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut id) },
        "AudioHardwareCreateProcessTap",
    )?;
    let format = get(
        id,
        kAudioTapPropertyFormat,
        kAudioObjectPropertyScopeGlobal,
        unsafe { std::mem::zeroed::<AudioStreamBasicDescription>() },
    )?;
    Ok(Tap { id, uid, format })
}

pub fn destroy_tap(id: u32) -> Result<()> {
    check(
        unsafe { AudioHardwareDestroyProcessTap(id) },
        "AudioHardwareDestroyProcessTap",
    )
}

fn key(k: &std::ffi::CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap())
}

fn dict(pairs: &[(&std::ffi::CStr, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| key(k)).collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&key_refs, &values)
}

/// A private aggregate device: the microphone as main (clock) sub-device, the
/// tap in the tap list with drift compensation, tap started automatically.
pub fn create_aggregate(mic_uid: &str, tap_uid: &str) -> Result<u32> {
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
        "com.sailvai.anchovy.audio-spike.{}",
        std::process::id()
    ));
    let name = NSString::from_str("Anchovy audio spike");
    let description = dict(&[
        (kAudioAggregateDeviceUIDKey, &uid),
        (kAudioAggregateDeviceNameKey, &name),
        (kAudioAggregateDeviceMainSubDeviceKey, &mic),
        (kAudioAggregateDeviceIsPrivateKey, &yes),
        (
            kAudioAggregateDeviceIsStackedKey,
            &NSNumber::new_bool(false),
        ),
        (kAudioAggregateDeviceTapAutoStartKey, &yes),
        (kAudioAggregateDeviceSubDeviceListKey, &sub_devices),
        (kAudioAggregateDeviceTapListKey, &taps),
    ]);
    // NSDictionary is toll-free bridged to CFDictionary.
    let cf = unsafe { &*(Retained::as_ptr(&description) as *const CFDictionary) };
    let mut id = 0u32;
    check(
        unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut id)) },
        "AudioHardwareCreateAggregateDevice",
    )?;
    Ok(id)
}

pub fn destroy_aggregate(id: u32) -> Result<()> {
    check(
        unsafe { AudioHardwareDestroyAggregateDevice(id) },
        "AudioHardwareDestroyAggregateDevice",
    )
}

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    context: *mut c_void,
) -> i32 {
    // Spike only: a mutex and a growing Vec are not real-time safe.
    let streams = unsafe { &*(context as *const Mutex<Vec<Stream>>) };
    let list = input.as_ptr();
    let count = unsafe { (*list).mNumberBuffers } as usize;
    let first = unsafe { (*list).mBuffers.as_ptr() };
    let buffers: Vec<&[f32]> = (0..count)
        .map(|i| unsafe {
            let b = &*first.add(i);
            if b.mData.is_null() {
                &[][..]
            } else {
                std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4)
            }
        })
        .collect();
    if let Ok(mut streams) = streams.try_lock() {
        append_cycle(&mut streams, &buffers);
    }
    0
}

/// Runs the device's input for `duration` and returns what arrived.
pub fn record(device: u32, streams: Vec<Stream>, duration: Duration) -> Result<Vec<Stream>> {
    let shared: Box<Mutex<Vec<Stream>>> = Box::new(Mutex::new(streams));
    let context = &*shared as *const Mutex<Vec<Stream>> as *mut c_void;
    let mut proc_id: AudioDeviceIOProcID = None;
    check(
        unsafe {
            AudioDeviceCreateIOProcID(device, Some(io_proc), context, NonNull::from(&mut proc_id))
        },
        "AudioDeviceCreateIOProcID",
    )?;
    let started = check(
        unsafe { AudioDeviceStart(device, proc_id) },
        "AudioDeviceStart",
    );
    let stopped = if started.is_ok() {
        std::thread::sleep(duration);
        check(
            unsafe { AudioDeviceStop(device, proc_id) },
            "AudioDeviceStop",
        )
    } else {
        Ok(())
    };
    let destroyed = check(
        unsafe { AudioDeviceDestroyIOProcID(device, proc_id) },
        "AudioDeviceDestroyIOProcID",
    );
    let teardown = IoTeardown {
        started: started.is_ok(),
        stopped: started.is_ok() && stopped.is_ok(),
        destroyed: destroyed.is_ok(),
    };
    let outcome = started.and(stopped).and(destroyed);
    if !may_free_io_context(teardown) {
        // The IO proc may still be called with `context`: keep it alive for good.
        Box::leak(shared);
        return Err(outcome
            .err()
            .unwrap_or_else(|| "IO proc teardown failed".into()));
    }
    outcome?;
    Ok(shared.into_inner().unwrap())
}
