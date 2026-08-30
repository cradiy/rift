mod entry;
mod location;

pub use entry::{Entry, EntryCategory, EntryKind};
pub use location::{
    Location, LocationKind, NavigationSnapshot, is_trash_location, trash_location_path,
};
