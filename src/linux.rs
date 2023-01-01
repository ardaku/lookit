// Copyright © 2021-2023 The Lookit Crate Developers
//
// Licensed under any of:
// - Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0)
// - Boost Software License, Version 1.0 (https://www.boost.org/LICENSE_1_0.txt)
// - MIT License (https://mit-license.org/)
// At your option (See accompanying files LICENSE_APACHE_2_0.txt,
// LICENSE_MIT.txt and LICENSE_BOOST_1_0.txt).  This file may not be copied,
// modified, or distributed except according to those terms.

use std::{
    ffi::CString,
    fs::{File, OpenOptions, ReadDir},
    mem::{self, MaybeUninit},
    os::{
        raw::{c_char, c_int, c_void},
        unix::{
            fs::OpenOptionsExt,
            io::{AsRawFd, RawFd},
        },
    },
};

use pasts::prelude::*;
use smelling_salts::epoll::Device;

use crate::{Found, Interface, Kind, Platform};

impl Interface for Platform {
    fn searcher(
        kind: Kind,
    ) -> Option<Box<dyn Notifier<Event = Found> + Unpin>> {
        Searcher::new(kind).map(
            |x| -> Box<dyn Notifier<Event = Found> + Unpin> { Box::new(x) },
        )
    }
}

impl Found {
    /// **`linux only`** Open read and write non-blocking device
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

    /// **`linux only`** Open read and write non-blocking device
    pub fn open(self) -> Result<RawFd, Self> {
        self.file_open().map(|x| x.as_raw_fd())
    }

    /// **`linux only`** Open read-only non-blocking device
    pub fn open_r(self) -> Result<RawFd, Self> {
        self.file_open_r().map(|x| x.as_raw_fd())
    }

    /// **`linux only`** Open write-only non-blocking device
    pub fn open_w(self) -> Result<RawFd, Self> {
        self.file_open_w().map(|x| x.as_raw_fd())
    }

    /// **`linux only`** Open read and write non-blocking File
    pub fn file_open(self) -> Result<File, Self> {
        self.open_flags(true, true)
    }

    /// **`linux only`** Open read-only non-blocking File
    pub fn file_open_r(self) -> Result<File, Self> {
        self.open_flags(true, false)
    }

    /// **`linux only`** Open write-only non-blocking File
    pub fn file_open_w(self) -> Result<File, Self> {
        self.open_flags(false, true)
    }
}

// Searcher

#[derive(Debug)]
struct Searcher {
    path: &'static str,
    prefix: &'static str,
    device: Device,
    read_dir: std::io::Result<ReadDir>,
}

impl Searcher {
    fn new(kind: Kind) -> Option<Self> {
        use Kind::*;
        match kind {
            Input() => Self::with("/dev/input/", "event"),
            Audio() => Self::with("/dev/snd/", "pcm"),
            Midi() => Self::with("/dev/snd/", "midi")
                .or_else(|| Self::with("/dev/", "midi")),
            Camera() => Self::with("/dev/", "video"),
        }
    }

    fn with(path: &'static str, prefix: &'static str) -> Option<Self> {
        let listen = unsafe { inotify_init1(0o2004000) };
        assert_ne!(-1, listen); // The only way this fails is some kind of OOM

        let dir = CString::new(path).unwrap();
        if unsafe { inotify_add_watch(listen, dir.into_raw(), 4) } == -1 {
            return None;
        }

        let read_dir = std::fs::read_dir(path);
        let device = Device::builder().input().watch(listen);
        let connector = Self {
            device,
            path,
            prefix,
            read_dir,
        };

        Some(connector)
    }
}

impl Notifier for Searcher {
    type Event = Found;

    fn poll_next(self: Pin<&mut Self>, exec: &mut Exec<'_>) -> Poll<Found> {
        let searcher = self.get_mut();

        // Check initial device iterator.
        if let Ok(ref mut read_dir) = searcher.read_dir {
            for file in read_dir.flatten() {
                let name = if let Ok(f) = file.file_name().into_string() {
                    f
                } else {
                    continue;
                };
                if let Some(file) = file.path().to_str() {
                    if name.starts_with(searcher.prefix) {
                        return Ready(Found(file.to_string()));
                    }
                }
            }
            searcher.read_dir = std::io::Result::Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "",
            ));
        }

        // Check for ready file descriptor.
        let fd = searcher.device.fd();

        if let Ready(()) = Pin::new(&mut searcher.device).poll_next(exec) {
            let mut ev = MaybeUninit::<InotifyEv>::uninit();
            if unsafe {
                read(fd, ev.as_mut_ptr().cast(), mem::size_of::<InotifyEv>())
            } > 0
            {
                let ev = unsafe { ev.assume_init() };
                let len = unsafe { strlen(&ev.name[0]) };
                let filename = String::from_utf8_lossy(&ev.name[..len]);
                if filename.starts_with(searcher.prefix) {
                    let path = format!("{}{filename}", searcher.path);
                    return Ready(Found(path));
                }
            }
        }

        Pending
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
