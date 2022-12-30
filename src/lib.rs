// Copyright © 2021-2022 The Lookit Crate Developers
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
//! manner.  Returns the `RawFd` equivalent for the target platform.
//!
//!  - Linux: inotify on /dev/*
//!  - Web: JavaScript event listeners
//!  - Others: TODO
//!
//! ## Getting Started
//! ```rust, no_run
#![doc = include_str!("../examples/hello.rs")]
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

#![warn(
    anonymous_parameters,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    nonstandard_style,
    rust_2018_idioms,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unused_extern_crates,
    unused_qualifications,
    variant_size_differences
)]

use std::{
    ffi::CString,
    fs::{File, OpenOptions, ReadDir},
    future::Future,
    mem::{self, MaybeUninit},
    os::{
        raw::{c_char, c_int, c_void},
        unix::{
            fs::OpenOptionsExt,
            io::{AsRawFd, RawFd},
        },
    },
    pin::Pin,
    task::{Context, Poll},
};

use smelling_salts::linux::{Device, Watcher};

/// Lookit future.  Becomes ready when a new device is created.
#[derive(Debug)]
pub struct Lookit(Option<(Device, Connector)>);

impl Lookit {
    //
    fn new(path: &'static str, prefix: &'static str) -> Option<Self> {
        let listen = unsafe { inotify_init1(0o2004000) };
        assert_ne!(-1, listen); // The only way this fails is some kind of OOM

        let dir = CString::new(path).unwrap();
        if unsafe { inotify_add_watch(listen, dir.into_raw(), 4) } == -1 {
            return None;
        }

        let read_dir = std::fs::read_dir(path);
        let connector = Connector {
            listen,
            path,
            prefix,
            read_dir,
        };

        Some(Self(Some((
            Device::new(listen, Watcher::new().input(), true),
            connector,
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
            // Check initial device iterator.
            if let Ok(ref mut read_dir) = device.1.read_dir {
                for file in read_dir.flatten() {
                    let name = if let Ok(f) = file.file_name().into_string() {
                        f
                    } else {
                        continue;
                    };
                    if let Some(file) = file.path().to_str() {
                        if name.starts_with(device.1.prefix) {
                            return Poll::Ready(It(file.to_string()));
                        }
                    }
                }
                device.1.read_dir = std::io::Result::Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "",
                ));
            }

            // Check for ready file descriptor.
            let fd = device.1.listen;
            if let Poll::Ready(()) = Pin::new(&mut device.0).poll(cx) {
                let mut ev = MaybeUninit::<InotifyEv>::uninit();
                if unsafe {
                    read(
                        fd,
                        ev.as_mut_ptr().cast(),
                        mem::size_of::<InotifyEv>(),
                    )
                } > 0
                {
                    let ev = unsafe { ev.assume_init() };
                    let len = unsafe { strlen(&ev.name[0]) };
                    let filename = String::from_utf8_lossy(&ev.name[..len]);
                    if filename.starts_with(device.1.prefix) {
                        let path = format!("{}{}", device.1.path, filename);
                        return Poll::Ready(It(path));
                    }
                }
            }
        }
        Poll::Pending
    }
}

/// Device found by the Lookit struct.
#[derive(Debug)]
pub struct It(String);

impl It {
    /// Open read and write non-blocking device
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

    /// Open read and write non-blocking device
    pub fn open(self) -> Result<RawFd, Self> {
        self.file_open().map(|x| x.as_raw_fd())
    }

    /// Open read-only non-blocking device
    pub fn open_r(self) -> Result<RawFd, Self> {
        self.file_open_r().map(|x| x.as_raw_fd())
    }

    /// Open write-only non-blocking device
    pub fn open_w(self) -> Result<RawFd, Self> {
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
    fn strlen(s: *const u8) -> usize;
}

#[derive(Debug)]
struct Connector {
    path: &'static str,
    prefix: &'static str,
    listen: RawFd,
    read_dir: std::io::Result<ReadDir>,
}
