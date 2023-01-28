// Copyright © 2021-2023 The Lookit Crate Developers
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
//! use lookit::Searcher;
//! use pasts::prelude::*;
//!
//! #[async_main::async_main]
//! async fn main(_spawner: impl Spawn) {
//!     let mut searcher = Searcher::with_camera();
//!     loop {
//!         let file = searcher.next().await;
//!         dbg!(file);
//!     }
//! }
//! ```
//!
//! ## Implementation
//! Input
//!  - inotify => /dev/input/event*
//!  - `window.addEventListener("gamepadconnected", function(e) { });`
//!
//! Audio
//!  - inotify => /dev/snd/pcm*
//!  - `navigator.mediaDevices.getUserMedia(constraints).then(function(s) {
//!    }).catch(function(denied_err) {})` // only one speakers connection ever
//!
//! MIDI
//!  - inotify => /dev/snd/midi*, if no /dev/snd then /dev/midi*
//!  - <https://developer.mozilla.org/en-US/docs/Web/API/MIDIAccess>
//!
//! Camera
//!  - inotify => /dev/video*
//!  - `navigator.mediaDevices.getUserMedia(constraints).then(function(s) {
//!    }).catch(function(denied_err) {})`

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

#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(target_os = "linux"), path = "mock.rs")]
mod platform;

use std::fmt;

use pasts::prelude::*;

use self::platform::Device;

/// Device kinds
enum Kind {
    Input(),
    Audio(),
    Midi(),
    Camera(),
}

enum Events {
    Read(),
    Write(),
    All(),
}

/// Platform implementation
struct Platform;

/// Interface should be implemented for each `Platform`
trait Interface {
    /// Create a searcher for a specific type of device
    fn searcher(kind: Kind)
        -> Option<Box<dyn Notifier<Event = Found> + Unpin>>;

    /// Try to watch a found device for both read+write events
    fn open(found: Found, events: Events) -> Result<Device, Found>;
}

/// Lookit [`Notifier`].  Lets you know when a device is [`Found`].
pub struct Searcher(Option<Box<dyn Notifier<Event = Found> + Unpin>>);

impl fmt::Debug for Searcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Searcher").finish_non_exhaustive()
    }
}

impl Searcher {
    /// Create new future checking for input devices.
    pub fn with_input() -> Self {
        Self(Platform::searcher(Kind::Input()))
    }

    /// Create new future checking for audio devices (speakers, microphones).
    pub fn with_audio() -> Self {
        Self(Platform::searcher(Kind::Audio()))
    }

    /// Create new future checking for MIDI devices.
    pub fn with_midi() -> Self {
        Self(Platform::searcher(Kind::Midi()))
    }

    /// Create new future checking for camera devices.
    pub fn with_camera() -> Self {
        Self(Platform::searcher(Kind::Camera()))
    }
}

impl Notifier for Searcher {
    type Event = Found;

    fn poll_next(mut self: Pin<&mut Self>, task: &mut Task<'_>) -> Poll<Found> {
        let Some(ref mut notifier) = &mut self.0 else { return Pending };

        Pin::new(notifier).poll_next(task)
    }
}

/// Device found by the [`Searcher`] notifier.
#[derive(Debug)]
pub struct Found(String);

impl Found {
    /// Connect to device (input + output)
    pub fn connect(self) -> Result<Device, Found> {
        Platform::open(self, Events::All())
    }

    /// Connect to device (input only)
    pub fn connect_input(self) -> Result<Device, Found> {
        Platform::open(self, Events::Read())
    }

    /// Connect to device (output only)
    pub fn connect_output(self) -> Result<Device, Found> {
        Platform::open(self, Events::Write())
    }
}
