use std::{
    ffi::CString,
    fs::{OpenOptions, ReadDir},
    io::Read,
    mem,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        raw::{c_char, c_int},
        unix::fs::OpenOptionsExt,
    },
};

use pasts::prelude::*;
use smelling_salts::Watch;

use crate::{Device, Events, Found, Interface, Kind, Platform};

// Inotify

/// struct inotify_event, from C.
#[repr(C)]
struct InotifyEv {
    /// Watch descriptor
    wd: RawFd,
    /// Mask describing event
    mask: u32,
    /// Unique cookie associating related events (for rename(2))
    cookie: u32,
    /// Size of following name field including null bytes
    len: u32,
}

extern "C" {
    fn inotify_init1(flags: c_int) -> RawFd;
    fn inotify_add_watch(fd: RawFd, path: *const c_char, mask: u32) -> c_int;
}

// Lookit interface

impl Interface for Platform {
    type Searcher = Searcher;

    fn searcher(kind: Kind) -> Option<Searcher> {
        Searcher::new(kind)
    }

    fn open(found: Found, events: Events) -> Result<Device, Found> {
        use Events::*;
        let device = match events {
            Read() => Device::new(found.open_r()?, Watch::INPUT),
            Write() => Device::new(found.open_w()?, Watch::OUTPUT),
            All() => Device::new(found.open()?, Watch::INPUT.output()),
        };

        Ok(device)
    }
}

impl Found {
    /// Open read and write non-blocking device
    fn open_flags(mut self, read: bool, write: bool) -> Result<OwnedFd, Self> {
        if let Ok(file) = OpenOptions::new()
            .read(read)
            .write(write)
            .custom_flags(2048)
            .open(self.0.get_mut())
        {
            Ok(file.into())
        } else {
            Err(self)
        }
    }

    /// Open read and write non-blocking
    fn open(self) -> Result<OwnedFd, Self> {
        self.open_flags(true, true)
    }

    /// Open read-only non-blocking
    fn open_r(self) -> Result<OwnedFd, Self> {
        self.open_flags(true, false)
    }

    /// Open write-only non-blocking
    fn open_w(self) -> Result<OwnedFd, Self> {
        self.open_flags(false, true)
    }
}

// Searcher

#[derive(Debug)]
pub(super) struct Searcher {
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
        let listen = unsafe { OwnedFd::from_raw_fd(listen) };

        let dir = CString::new(path).unwrap();
        if unsafe { inotify_add_watch(listen.as_raw_fd(), dir.into_raw(), 4) }
            == -1
        {
            return None;
        }

        let read_dir = std::fs::read_dir(path);
        let device = Device::new(listen, Watch::INPUT);
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

    fn poll_next(self: Pin<&mut Self>, task: &mut Task<'_>) -> Poll<Found> {
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
                        return Ready(Found(file.to_string().into()));
                    }
                }
            }
            searcher.read_dir = std::io::Result::Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "",
            ));
        }

        // Check for ready file descriptor.
        while let Ready(()) = Pin::new(&mut searcher.device).poll_next(task) {
            let mut bytes = [0; mem::size_of::<InotifyEv>()];

            if let Err(e) = searcher.device.read_exact(&mut bytes) {
                dbg!(e);
                continue;
            }

            let inotify_ev: InotifyEv = unsafe { mem::transmute(bytes) };
            let len = inotify_ev.len.try_into().unwrap_or(usize::MAX);
            let mut bytes = vec![0; len];

            if let Err(e) = searcher.device.read_exact(bytes.as_mut_slice()) {
                dbg!(e);
                continue;
            }

            let bytes = bytes
                .as_slice()
                .split(|n| *n == b'\0')
                .next()
                .unwrap_or_default();
            let filename = String::from_utf8_lossy(bytes);

            if filename.starts_with(searcher.prefix) {
                let path = format!("{}{filename}", searcher.path);
                return Ready(Found(path.into()));
            }
        }

        Pending
    }
}
