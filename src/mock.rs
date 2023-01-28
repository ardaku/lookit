// Copyright © 2021-2023 The Lookit Crate Developers
//
// Licensed under any of:
// - Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0)
// - Boost Software License, Version 1.0 (https://www.boost.org/LICENSE_1_0.txt)
// - MIT License (https://mit-license.org/)
// At your option (See accompanying files LICENSE_APACHE_2_0.txt,
// LICENSE_MIT.txt and LICENSE_BOOST_1_0.txt).  This file may not be copied,
// modified, or distributed except according to those terms.

use pasts::prelude::*;

use crate::{Device, Events, Found, Interface, Kind, Platform};

impl Interface for Platform {
    fn searcher(
        _kind: Kind,
    ) -> Option<Box<dyn Notifier<Event = Found> + Unpin>> {
        None
    }

    fn open(found: Found, _events: Events) -> Result<Device, Found> {
        Err(found)
    }
}
