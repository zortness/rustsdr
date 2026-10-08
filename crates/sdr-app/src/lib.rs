//! Presentation-independent application services.
#[cfg(feature = "audio")]
pub mod audio;
pub mod controller;
pub mod digital;
pub mod engine;
pub mod memories;
pub mod mode_settings;
pub mod practice;
#[cfg(feature = "midi")]
pub mod practice_midi;
pub mod preferences;
pub mod scanning;
pub mod spectrum_view;
pub mod tuning;
pub mod waterfall_labels;

pub mod vor;
