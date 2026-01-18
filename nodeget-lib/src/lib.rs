#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::await_holding_lock,
    dead_code
)]

pub mod config;
pub mod monitoring;
pub mod utils;
pub mod task;
