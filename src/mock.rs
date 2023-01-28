use pasts::prelude::*;

use crate::{Device, Events, Found, Interface, Kind, Platform};

impl Interface for Platform {
    type Searcher = BoxNotifier<'static, Found>;

    fn searcher(_kind: Kind) -> Option<BoxNotifier<'static, Found>> {
        None
    }

    fn open(found: Found, _events: Events) -> Result<Device, Found> {
        Err(found)
    }
}
