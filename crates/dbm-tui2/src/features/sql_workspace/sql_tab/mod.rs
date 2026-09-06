//! `sql_tab` parent feature: owns tab management and contains the child
//! modules `editor`, `results` and `history`.

pub mod editor;
pub mod effect;
pub mod history;
pub mod input;
pub mod intent;
pub mod layout;
pub mod msg;
pub mod results;
pub mod session;
pub mod splitter;
pub mod state;
pub mod tab;
pub mod update;
pub mod view;
