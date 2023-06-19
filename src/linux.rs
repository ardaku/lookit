use std::{
    ffi::CString,
    fs::{OpenOptions, ReadDir},
    io::Read,
    mem::{self, MaybeUninit},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        raw::{c_char, c_int, c_uint, c_ulong},
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
    buffer: Vec<u8>,
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
        // https://github.com/torvalds/linux/blob/dbad9ce9397ef7f891b4ff44bad694add673c1a1/include/uapi/linux/inotify.h#L29

        const IN_NONBLOCK: c_int = 0o4000;
        const IN_CLOEXEC: c_int = 0o2000000;

        const IN_ATTRIB: u32 = 0x004;
        const IN_CREATE: u32 = 0x100;
        const IN_DELETE: u32 = 0x200;

        let listen = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
        assert_ne!(-1, listen); // The only way this fails is some kind of OOM
        let listen = unsafe { OwnedFd::from_raw_fd(listen) };

        let dir = CString::new(path).unwrap();
        if unsafe {
            inotify_add_watch(
                listen.as_raw_fd(),
                dir.as_c_str().as_ptr(),
                IN_ATTRIB | IN_CREATE | IN_DELETE,
            )
        } == -1
        {
            return None;
        }

        let read_dir = std::fs::read_dir(path);
        let device = Device::new(listen, Watch::INPUT);
        let buffer = Vec::new();
        let connector = Self {
            device,
            path,
            prefix,
            read_dir,
            buffer,
        };

        Some(connector)
    }

    fn find(&mut self) -> Option<Found> {
        if self.buffer.is_empty() {
            return None;
        }

        let begin: [u8; mem::size_of::<InotifyEv>()] = self.buffer
            [..mem::size_of::<InotifyEv>()]
            .try_into()
            .unwrap();
        let inotify_ev: InotifyEv = unsafe { mem::transmute(begin) };
        let len = inotify_ev.len.try_into().unwrap_or(usize::MAX);
        let bytes = &self.buffer[mem::size_of::<InotifyEv>()..][..len];
        let bytes = bytes.split(|n| *n == b'\0').next().unwrap_or_default();
        let filename = String::from_utf8_lossy(bytes);

        if filename.starts_with(self.prefix) {
            let path = format!("{}{filename}", self.path);

            self.buffer.drain(..mem::size_of::<InotifyEv>() + len);

            return Some(Found(path.into()));
        }

        self.buffer.drain(..mem::size_of::<InotifyEv>() + len);

        self.find()
    }
}

impl Notify for Searcher {
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

        if let Some(found) = searcher.find() {
            return Ready(found);
        }

        // Check for ready file descriptor.
        while let Ready(()) = Pin::new(&mut searcher.device).poll_next(task) {
            // https://github.com/torvalds/linux/blob/dbad9ce9397ef7f891b4ff44bad694add673c1a1/include/uapi/asm-generic/ioctls.h#L46
            const FIONREAD: c_ulong = 0x541B;
            extern "C" {
                fn ioctl(fd: RawFd, req: c_ulong, len: *mut c_uint) -> c_int;
            }
            let mut len = MaybeUninit::uninit();
            let ret = unsafe {
                ioctl(searcher.device.as_raw_fd(), FIONREAD, len.as_mut_ptr())
            };
            assert!(ret >= 0);
            let len = unsafe { len.assume_init() };

            searcher
                .buffer
                .resize(len.try_into().unwrap_or(usize::MAX), 0);

            if let Err(e) = searcher.device.read_exact(&mut searcher.buffer) {
                dbg!(e);
            }

            if let Some(found) = searcher.find() {
                return Ready(found);
            }
        }

        Pending
    }
}
