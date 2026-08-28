//! rusty_erasure-alloc — the allocator seam.
//!
//! Deliverable crates (and only deliverables) declare:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: rusty_erasure_alloc::HouseAllocator = rusty_erasure_alloc::house_allocator();
//! ```
//!
//! Libraries never touch this crate, and never depend on `rusty_alloc-api`
//! directly — the pin to the house allocator lives here, once, so swapping it
//! is a one-line change for every deliverable at once (mission plan §4).

#![deny(missing_docs)]

pub use rusty_alloc_api::RustyAlloc as HouseAllocator;

/// The one allocator every deliverable in this workspace declares.
pub const fn house_allocator() -> HouseAllocator {
    rusty_alloc_api::RustyAlloc
}
