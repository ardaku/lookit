use pasts::prelude::*;

use crate::{Device, Events, Found, Interface, Kind, Platform};

impl Interface for Platform {
    type Searcher = BoxNotify<'static, Found>;

    fn searcher(_kind: Kind) -> Option<BoxNotify<'static, Found>> {
        None
    }

    fn open(found: Found, _events: Events) -> Result<Device, Found> {
        Err(found)
    }
}
