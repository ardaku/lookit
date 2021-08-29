// Copyright © 2021 The Lookit Crate Developers
//
// Licensed under any of:
// - Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0)
// - Boost Software License, Version 1.0 (https://www.boost.org/LICENSE_1_0.txt)
// - MIT License (https://mit-license.org/)
// At your option (See accompanying files LICENSE_APACHE_2_0.txt,
// LICENSE_MIT.txt and LICENSE_BOOST_1_0.txt).  This file may not be copied,
// modified, or distributed except according to those terms.
//
//! The "Lookit!" crate checks for new devices in a cross-platform asynchronous
//! manner.  Returns the smelling_salts `RawDevice` for the target platform.
//!
//!  - Linux: inotify on /dev/*
//!  - Web: JavaScript event listeners
//!  - Others: TODO
//!
//! ## Getting Started
//! ```rust
//! ```
//!
//! ## Implementation
//! Input
//!  - inotify => /dev/input/event*
//!  - window.addEventListener("gamepadconnected", function(e) { });
//!
//! Audio
//!  - inotify => /dev/snd/pcm*
//!  - navigator.mediaDevices.getUserMedia(constraints).then(function(s) { }).catch(function(denied_err) {}) // only one speakers connection ever
//!
//! MIDI
//!  - inotify => /dev/snd/midi*, if no /dev/snd then /dev/midi*
//!  - https://developer.mozilla.org/en-US/docs/Web/API/MIDIAccess
//!
//! Camera
//!  - inotify => /dev/video*
//!  - navigator.mediaDevices.getUserMedia(constraints).then(function(s) { }).catch(function(denied_err) {})

use flume::Sender;
use smelling_salts::linux::{Device, Driver, RawDevice, Watcher};
use std::ffi::CString;
use std::fs::File;
use std::future::Future;
use std::mem::{self, MaybeUninit};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::{FromRawFd, RawFd};
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};

/// Lookit future.  Becomes ready when a new device is created.
pub struct Lookit(Device<It>);

impl Lookit {
    //
    fn new(path: &'static str, prefix: &'static str) -> Option<Self> {
        let driver = driver();

        let listen = unsafe { inotify_init1(0o2004000) };
        assert_ne!(-1, listen); // The only way this fails is some kind of OOM

        let dir = CString::new(path).unwrap();
        if unsafe { inotify_add_watch(listen, dir.into_raw(), 4) } == -1 {
            return None;
        }

        Some(Self(driver.device(
            |sender| Connector {
                sender,
                listen,
                path,
                prefix,
            },
            listen,
            Connector::callback,
            Watcher::new().input(),
        )))
    }

    /// Create new future checking for input devices.
    pub fn with_input() -> Option<Self> {
        Self::new("/dev/input/", "event")
    }

    /// Create new future checking for audio devices (speakers, microphones).
    pub fn with_audio() -> Option<Self> {
        Self::new("/dev/snd/", "pcm")
    }

    /// Create new future checking for MIDI devices.
    pub fn with_midi() -> Option<Self> {
        Self::new("/dev/snd/", "midi").or_else(|| Self::new("/dev/", "midi"))
    }

    /// Create new future checking for camera devices.
    pub fn with_camera() -> Option<Self> {
        Self::new("/dev/", "video")
    }
}

impl Future for Lookit {
    type Output = It;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().0).poll(cx)
    }
}

/// Device found by the Lookit struct.
pub struct It(String);

impl It {
    /// Open read and write non-blocking RawDevice
    pub fn open_flags(self, flags: c_int) -> Option<RawDevice> {
        let filename = CString::new(self.0).unwrap();
        let fd = unsafe { open(filename.as_ptr(), flags) };
        if fd == -1 {
            return None;
        }
        Some(fd)
    }

    /// Open read and write non-blocking RawDevice
    pub fn open(self) -> Option<RawDevice> {
        self.open_flags(0o2004002)
    }

    /// Open read-only non-blocking RawDevice
    pub fn open_r(self) -> Option<RawDevice> {
        self.open_flags(0o2004000)
    }

    /// Open write-only non-blocking RawDevice
    pub fn open_w(self) -> Option<RawDevice> {
        self.open_flags(0o2004001)
    }

    /// Open read and write non-blocking RawDevice
    pub fn file_open(self) -> Option<File> {
        self.open()
            .map(|raw_fd| unsafe { File::from_raw_fd(raw_fd) })
    }

    /// Open read-only non-blocking RawDevice
    pub fn file_open_r(self) -> Option<File> {
        self.open_r()
            .map(|raw_fd| unsafe { File::from_raw_fd(raw_fd) })
    }

    /// Open write-only non-blocking RawDevice
    pub fn file_open_w(self) -> Option<File> {
        self.open_w()
            .map(|raw_fd| unsafe { File::from_raw_fd(raw_fd) })
    }
}

// Inotify

fn driver() -> &'static Driver {
    static mut DRIVER: MaybeUninit<Driver> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();
    unsafe {
        ONCE.call_once(|| DRIVER = MaybeUninit::new(Driver::new()));
        &*DRIVER.as_ptr()
    }
}

#[repr(C)]
struct InotifyEv {
    // struct inotify_event, from C.
    wd: c_int, /* Watch descriptor */
    mask: u32, /* Mask describing event */
    cookie: u32, /* Unique cookie associating related
               events (for rename(2)) */
    len: u32,        /* Size of name field */
    name: [u8; 256], /* Optional null-terminated name */
}

extern "C" {
    fn inotify_init1(flags: c_int) -> RawFd;
    fn inotify_add_watch(fd: RawFd, path: *const c_char, mask: u32) -> c_int;
    fn read(fd: RawFd, buf: *mut c_void, count: usize) -> isize;
    fn close(fd: RawFd) -> c_int;
    fn strlen(s: *const u8) -> usize;
    fn open(pathname: *const c_char, flags: c_int) -> c_int;
}

struct Connector {
    path: &'static str,
    prefix: &'static str,
    listen: RawFd,
    sender: Sender<It>,
}

impl Connector {
    unsafe fn callback(&mut self) -> Option<()> {
        let mut ev = MaybeUninit::<InotifyEv>::zeroed();
        if read(
            self.listen,
            ev.as_mut_ptr().cast(),
            mem::size_of::<InotifyEv>(),
        ) == mem::size_of::<InotifyEv>() as _
        {
            let ev = ev.assume_init();
            let len = strlen(&ev.name[0]);
            let filename = String::from_utf8_lossy(&ev.name[..len]);
            if filename.starts_with(self.prefix) {
                let path = format!("{}{}", self.path, filename);
                if self.sender.send(It(path)).is_err() {
                    driver().discard(self.listen);
                    let ret = close(self.listen);
                    assert_eq!(0, ret);
                    return None;
                }
            }
        }
        Some(())
    }
}
