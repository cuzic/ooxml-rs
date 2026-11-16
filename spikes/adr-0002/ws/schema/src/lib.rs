//! Stand-in for generated code (crate `ooxml-schema`).
cfg_select! {
    feature = "sml" => { pub mod sml; }
    _ => {}
}
