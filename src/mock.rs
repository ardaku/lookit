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
