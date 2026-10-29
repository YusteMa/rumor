//! Which output we are on (headphones or speakers), what to do when it
//! changes (SPEC §3.3), and the Windows notifications that report changes:
//! `IMMNotificationClient` for the default output and suspend/resume.
//!
//! The policy is pure and tested here. The notification callbacks only forward
//! a `SystemEvent`; all the work happens on the engine's control thread.

use serde::{Deserialize, Serialize};

/// What kind of output an endpoint is, as far as the policy cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputKind {
    /// Headphones, headsets, earpieces: only the listener hears it.
    Personal,
    /// Speakers, HDMI/TVs, line out, virtual devices and anything unknown.
    /// Treated as public so noise never ends up in the room by surprise
    /// (CLAUDE.md rule 10).
    Public,
}

impl OutputKind {
    /// Windows reports this as `PKEY_AudioEndpoint_FormFactor`; `cpal` exposes
    /// it as `DeviceType`.
    pub fn from_device_type(device_type: cpal::DeviceType) -> Self {
        use cpal::DeviceType as T;
        match device_type {
            T::Headphones | T::Headset | T::Earpiece | T::Handset | T::HearingAid => Self::Personal,
            _ => Self::Public,
        }
    }
}

/// An output endpoint, identified by its stable Windows endpoint id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub kind: OutputKind,
}

/// The "If headphones are disconnected" setting (SPEC §3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputChangePolicy {
    /// Like a phone: unplugging headphones or losing Bluetooth stops the
    /// noise; switching between headphones keeps playing.
    #[default]
    PauseOnSpeakers,
    /// Any output change stops the noise.
    PauseAlways,
    /// Reconnect to the new output and keep playing.
    KeepPlaying,
}

/// Why the engine paused on its own (the panel shows a message for each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    HeadphonesDisconnected,
    OutputChanged,
    NoOutput,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    Pause(PauseReason),
}

/// What playing noise should do when the output moves from `from` to `to`
/// (`None` meaning there is no output at all).
pub fn on_output_change(
    policy: OutputChangePolicy,
    from: Option<&Endpoint>,
    to: Option<&Endpoint>,
) -> Verdict {
    let from_personal = from.is_some_and(|e| e.kind == OutputKind::Personal);
    let Some(to) = to else {
        return Verdict::Pause(if from_personal {
            PauseReason::HeadphonesDisconnected
        } else {
            PauseReason::NoOutput
        });
    };
    let Some(from) = from else {
        // Nothing was playing anywhere yet; the user asked for sound.
        return Verdict::Continue;
    };
    if from.id == to.id {
        return Verdict::Continue;
    }
    let to_public = to.kind == OutputKind::Public;
    let reason = if from_personal && to_public {
        PauseReason::HeadphonesDisconnected
    } else {
        PauseReason::OutputChanged
    };
    match policy {
        OutputChangePolicy::KeepPlaying => Verdict::Continue,
        OutputChangePolicy::PauseAlways => Verdict::Pause(reason),
        OutputChangePolicy::PauseOnSpeakers if from_personal && to_public => Verdict::Pause(reason),
        OutputChangePolicy::PauseOnSpeakers => Verdict::Continue,
    }
}

/// Whether a pause should end because headphones came back ("Resume when
/// headphones reconnect", off by default).
pub fn resumes_on(
    resume_on_reconnect: bool,
    paused_for: Option<PauseReason>,
    to: Option<&Endpoint>,
) -> bool {
    resume_on_reconnect
        && paused_for == Some(PauseReason::HeadphonesDisconnected)
        && to.is_some_and(|e| e.kind == OutputKind::Personal)
}

/// Something the engine should react to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    /// The default output changed, or a device appeared, vanished or was
    /// unplugged. The engine re-reads the default and compares.
    OutputChanged,
    /// The PC is about to sleep or hibernate.
    Suspending,
    /// The PC woke up.
    Resumed,
}

#[cfg(windows)]
pub use win::SystemWatch;

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::sync::Arc;

    use windows::Win32::Foundation::{HANDLE, PROPERTYKEY};
    use windows::Win32::Media::Audio::{
        DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Power::{
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY, PowerRegisterSuspendResumeNotification,
        PowerUnregisterSuspendResumeNotification,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
    };
    use windows::core::{PCWSTR, implement};

    use super::SystemEvent;

    type Notify = Arc<dyn Fn(SystemEvent) + Send + Sync>;

    /// Keeps the Windows registrations alive; dropping it unregisters them.
    /// Create and drop it on the same thread.
    pub struct SystemWatch {
        enumerator: IMMDeviceEnumerator,
        client: IMMNotificationClient,
        power: Option<HPOWERNOTIFY>,
        // Windows holds raw pointers to these until `power` is unregistered.
        _power_params: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
        _power_context: Box<Notify>,
        com_initialized: bool,
    }

    impl SystemWatch {
        pub fn start(
            notify: impl Fn(SystemEvent) + Send + Sync + 'static,
        ) -> windows::core::Result<Self> {
            // Same apartment as cpal uses on this thread.
            let com_initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let notify: Notify = Arc::new(notify);

            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let client: IMMNotificationClient = Client {
                notify: notify.clone(),
            }
            .into();
            unsafe { enumerator.RegisterEndpointNotificationCallback(&client)? };

            let power_context = Box::new(notify);
            let mut power_params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                Callback: Some(power_callback),
                Context: &*power_context as *const Notify as *mut c_void,
            });
            let power = register_power(&mut power_params);
            if power.is_none() {
                log::warn!("suspend/resume notifications unavailable");
            }

            Ok(Self {
                enumerator,
                client,
                power,
                _power_params: power_params,
                _power_context: power_context,
                com_initialized,
            })
        }
    }

    impl Drop for SystemWatch {
        fn drop(&mut self) {
            unsafe {
                // Both calls wait for in-flight callbacks to finish.
                let _ = self
                    .enumerator
                    .UnregisterEndpointNotificationCallback(&self.client);
                if let Some(handle) = self.power.take() {
                    let _ = PowerUnregisterSuspendResumeNotification(handle);
                }
                if self.com_initialized {
                    CoUninitialize();
                }
            }
        }
    }

    fn register_power(params: &mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS) -> Option<HPOWERNOTIFY> {
        let mut handle: *mut c_void = std::ptr::null_mut();
        let status = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(params as *mut _ as *mut c_void),
                &mut handle,
            )
        };
        status.is_ok().then_some(HPOWERNOTIFY(handle as isize))
    }

    unsafe extern "system" fn power_callback(
        context: *const c_void,
        kind: u32,
        _setting: *const c_void,
    ) -> u32 {
        // SAFETY: `context` is the boxed `Notify` owned by `SystemWatch`, which
        // unregisters this callback before freeing it.
        let notify = unsafe { &*(context as *const Notify) };
        match kind {
            PBT_APMSUSPEND => notify(SystemEvent::Suspending),
            PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => notify(SystemEvent::Resumed),
            _ => {}
        }
        0
    }

    #[implement(IMMNotificationClient)]
    struct Client {
        notify: Notify,
    }

    // Called on Windows' own threads: forward and return, never block.
    impl IMMNotificationClient_Impl for Client_Impl {
        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            role: ERole,
            _id: &PCWSTR,
        ) -> windows::core::Result<()> {
            // cpal's default output is the eConsole one.
            if flow == eRender && role == eConsole {
                (self.notify)(SystemEvent::OutputChanged);
            }
            Ok(())
        }

        fn OnDeviceStateChanged(
            &self,
            _id: &PCWSTR,
            _state: DEVICE_STATE,
        ) -> windows::core::Result<()> {
            (self.notify)(SystemEvent::OutputChanged);
            Ok(())
        }

        fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
            (self.notify)(SystemEvent::OutputChanged);
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _id: &PCWSTR,
            _key: &PROPERTYKEY,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }
}

/// Non-Windows builds (CI tooling only): no notifications, the engine's
/// periodic default-device check still catches changes.
#[cfg(not(windows))]
pub struct SystemWatch;

#[cfg(not(windows))]
impl SystemWatch {
    pub fn start(
        _notify: impl Fn(SystemEvent) + Send + Sync + 'static,
    ) -> Result<Self, std::io::Error> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, kind: OutputKind) -> Endpoint {
        Endpoint {
            id: id.into(),
            name: id.into(),
            kind,
        }
    }

    const ALL: [OutputChangePolicy; 3] = [
        OutputChangePolicy::PauseOnSpeakers,
        OutputChangePolicy::PauseAlways,
        OutputChangePolicy::KeepPlaying,
    ];

    #[test]
    fn default_policy_pauses_when_headphones_give_way_to_speakers() {
        let phones = endpoint("phones", OutputKind::Personal);
        let speakers = endpoint("speakers", OutputKind::Public);
        assert_eq!(
            on_output_change(
                OutputChangePolicy::default(),
                Some(&phones),
                Some(&speakers)
            ),
            Verdict::Pause(PauseReason::HeadphonesDisconnected)
        );
    }

    #[test]
    fn default_policy_keeps_playing_between_headphones_and_from_speakers() {
        let p = OutputChangePolicy::PauseOnSpeakers;
        let wired = endpoint("wired", OutputKind::Personal);
        let bt = endpoint("bt", OutputKind::Personal);
        let speakers = endpoint("speakers", OutputKind::Public);
        let hdmi = endpoint("hdmi", OutputKind::Public);
        assert_eq!(
            on_output_change(p, Some(&wired), Some(&bt)),
            Verdict::Continue
        );
        assert_eq!(
            on_output_change(p, Some(&speakers), Some(&wired)),
            Verdict::Continue
        );
        assert_eq!(
            on_output_change(p, Some(&speakers), Some(&hdmi)),
            Verdict::Continue
        );
    }

    #[test]
    fn pause_always_pauses_on_any_change() {
        let p = OutputChangePolicy::PauseAlways;
        let wired = endpoint("wired", OutputKind::Personal);
        let bt = endpoint("bt", OutputKind::Personal);
        let speakers = endpoint("speakers", OutputKind::Public);
        assert_eq!(
            on_output_change(p, Some(&wired), Some(&bt)),
            Verdict::Pause(PauseReason::OutputChanged)
        );
        assert_eq!(
            on_output_change(p, Some(&speakers), Some(&wired)),
            Verdict::Pause(PauseReason::OutputChanged)
        );
        assert_eq!(
            on_output_change(p, Some(&wired), Some(&speakers)),
            Verdict::Pause(PauseReason::HeadphonesDisconnected)
        );
    }

    #[test]
    fn keep_playing_continues_even_onto_speakers() {
        let phones = endpoint("phones", OutputKind::Personal);
        let speakers = endpoint("speakers", OutputKind::Public);
        assert_eq!(
            on_output_change(
                OutputChangePolicy::KeepPlaying,
                Some(&phones),
                Some(&speakers)
            ),
            Verdict::Continue
        );
    }

    #[test]
    fn same_endpoint_is_never_a_change() {
        let phones = endpoint("phones", OutputKind::Personal);
        for p in ALL {
            assert_eq!(
                on_output_change(p, Some(&phones), Some(&phones)),
                Verdict::Continue
            );
        }
    }

    #[test]
    fn losing_every_output_always_pauses() {
        let phones = endpoint("phones", OutputKind::Personal);
        let speakers = endpoint("speakers", OutputKind::Public);
        for p in ALL {
            assert_eq!(
                on_output_change(p, Some(&phones), None),
                Verdict::Pause(PauseReason::HeadphonesDisconnected)
            );
            assert_eq!(
                on_output_change(p, Some(&speakers), None),
                Verdict::Pause(PauseReason::NoOutput)
            );
        }
    }

    #[test]
    fn resumes_only_when_enabled_after_an_unplug_and_onto_headphones() {
        let phones = endpoint("phones", OutputKind::Personal);
        let speakers = endpoint("speakers", OutputKind::Public);
        let unplugged = Some(PauseReason::HeadphonesDisconnected);
        assert!(resumes_on(true, unplugged, Some(&phones)));
        assert!(!resumes_on(false, unplugged, Some(&phones)));
        assert!(!resumes_on(true, unplugged, Some(&speakers)));
        assert!(!resumes_on(true, None, Some(&phones)));
        assert!(!resumes_on(
            true,
            Some(PauseReason::Suspended),
            Some(&phones)
        ));
    }

    #[test]
    fn device_types_map_to_personal_or_public() {
        use cpal::DeviceType as T;
        for t in [
            T::Headphones,
            T::Headset,
            T::Earpiece,
            T::Handset,
            T::HearingAid,
        ] {
            assert_eq!(
                OutputKind::from_device_type(t),
                OutputKind::Personal,
                "{t:?}"
            );
        }
        for t in [T::Speaker, T::Dock, T::Virtual, T::Unknown] {
            assert_eq!(OutputKind::from_device_type(t), OutputKind::Public, "{t:?}");
        }
    }
}
