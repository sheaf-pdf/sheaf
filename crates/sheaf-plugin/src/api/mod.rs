//! The WIT world this host serves, `sheaf:plugin@0.4.0`, bound once, and
//! the host side of its interfaces, a module each. Geometry crosses in
//! PageSpace as sheaf keeps it; a page's view matrices go with it. Reads
//! go through the call's transaction ([`HostState::tx`]), recording what
//! the call decides by and seeing its own edits; edits cross as Op JSON
//! (`edits`), lowered into it.
//!
//! [`HostState::tx`]: crate::runtime::HostState::tx

mod assets;
mod convert;
mod document;
mod edits;
mod progress;
mod render;
mod storage;
mod text;
mod ui;

mod bindings {
    wasmtime::component::bindgen!({
        world: "plugin",
        path: "../../wit",
        with: {
            "sheaf:plugin/render.page-renderer": crate::render::PageRenderer,
            "sheaf:plugin/render.image": crate::render::RenderedImage,
            "sheaf:plugin/progress.task": crate::progress::Task,
        },
        additional_derives: [PartialEq],
    });
}

use bindings::sheaf::plugin as api;

pub use assets::MAX_UPLOAD_BYTES;
pub(crate) use assets::Upload;
pub(crate) use bindings::Plugin as World;
pub(crate) use convert::event_to_wit;

impl api::types::Host for crate::runtime::HostState {}
