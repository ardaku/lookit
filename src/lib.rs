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
//! ```rust, no_run
//! # use lookit::Lookit;
//! async fn run() {
//!     let mut lookit = Lookit::with_input().expect("no /dev/ access?");
//!
//!     loop {
//!         dbg!((&mut lookit).await);
//!     }
//! }
//!
//! pasts::block_on(run());
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
//!  - <https://developer.mozilla.org/en-US/docs/Web/API/MIDIAccess>
//!
//! Camera
//!  - inotify => /dev/video*
//!  - navigator.mediaDevices.getUserMedia(constraints).then(function(s) { }).catch(function(denied_err) {})

use flume::Sender;
use smelling_salts::linux::{Device, Driver, RawDevice, Watcher};
use std::ffi::CString;
use std::fs::File;
use std::fs::OpenOptions;
use std::future::Future;
use std::mem::{self, MaybeUninit};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};

/// Lookit future.  Becomes ready when a new device is created.
#[derive(Debug)]
pub struct Lookit(Option<Device<It>>);

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

        Some(Self(Some(driver.device(
            |sender| Connector {
                sender,
                listen,
                path,
                prefix,
            },
            listen,
            Connector::callback,
            Watcher::new().input(),
        ))))
    }

    fn pending() -> Self {
        Self(None)
    }

    /// Create new future checking for input devices.
    pub fn with_input() -> Self {
        Self::new("/dev/input/", "event").unwrap_or_else(Self::pending)
    }

    /// Create new future checking for audio devices (speakers, microphones).
    pub fn with_audio() -> Self {
        Self::new("/dev/snd/", "pcm").unwrap_or_else(Self::pending)
    }

    /// Create new future checking for MIDI devices.
    pub fn with_midi() -> Self {
        Self::new("/dev/snd/", "midi")
            .or_else(|| Self::new("/dev/", "midi"))
            .unwrap_or_else(Self::pending)
    }

    /// Create new future checking for camera devices.
    pub fn with_camera() -> Self {
        Self::new("/dev/", "video").unwrap_or_else(Self::pending)
    }
}

impl Future for Lookit {
    type Output = It;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(ref mut device) = self.get_mut().0 {
            Pin::new(device).poll(cx)
        } else {
            Poll::Pending
        }
    }
}

/// Device found by the Lookit struct.
#[derive(Debug)]
pub struct It(String);

impl It {
    /// Open read and write non-blocking RawDevice
    fn open_flags(self, read: bool, write: bool) -> Result<File, Self> {
        if let Ok(file) = OpenOptions::new()
            .read(read)
            .write(write)
            .custom_flags(2048)
            .open(&self.0)
        {
            Ok(file)
        } else {
            Err(self)
        }
    }

    /// Open read and write non-blocking RawDevice
    pub fn open(self) -> Result<RawDevice, Self> {
        self.file_open().map(|x| x.as_raw_fd())
    }

    /// Open read-only non-blocking RawDevice
    pub fn open_r(self) -> Result<RawDevice, Self> {
        self.file_open_r().map(|x| x.as_raw_fd())
    }

    /// Open write-only non-blocking RawDevice
    pub fn open_w(self) -> Result<RawDevice, Self> {
        self.file_open_w().map(|x| x.as_raw_fd())
    }

    /// Open read and write non-blocking File
    pub fn file_open(self) -> Result<File, Self> {
        self.open_flags(true, true)
    }

    /// Open read-only non-blocking File
    pub fn file_open_r(self) -> Result<File, Self> {
        self.open_flags(true, false)
    }

    /// Open write-only non-blocking File
    pub fn file_open_w(self) -> Result<File, Self> {
        self.open_flags(false, true)
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
        ) > 0
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
