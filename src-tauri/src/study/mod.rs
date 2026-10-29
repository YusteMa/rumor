//! Study module (SPEC §4.3b, phase 3b): stopwatch/Pomodoro, idle detection and
//! session history. Linked to the sound only through the coordinator in
//! `state.rs` (CLAUDE.md rule 12).

mod db;
mod idle;
mod machine;
