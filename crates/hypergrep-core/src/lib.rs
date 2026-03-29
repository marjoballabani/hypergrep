#![allow(
    clippy::collapsible_if,
    clippy::type_complexity,
    clippy::ptr_arg,
    clippy::if_same_then_else,
    clippy::manual_clamp,
    clippy::empty_line_after_doc_comments
)]

pub mod bloom;
pub mod graph;
pub mod index;
pub mod mental_model;
pub mod persist;
pub mod posting;
pub mod prefetch;
pub mod semantic;
pub mod structure;
pub mod trigram;
pub mod walker;
