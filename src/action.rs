//! The `Action` enum: the only channel through which application state changes.
//!
//! Input events and background-worker messages are both translated into actions,
//! and `App::update` is the single place that applies them. Rendering never
//! mutates state.
