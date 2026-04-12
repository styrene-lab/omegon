//! Integrated features — subsystems that participate in the agent runtime.
//!
//! Each feature implements the `Feature` trait from `omegon-traits` and is
//! registered with the `EventBus` during setup. Features are modules, not
//! separate crates — the trait IS the decoupling boundary.
//!
//! # Adding a new feature
//!
//! 1. Create a module in `features/` (e.g. `features/auto_compact.rs`)
//! 2. Define a struct implementing `Feature`
//! 3. Register it in `setup.rs` via `bus.register(Box::new(MyFeature::new()))`
//!
//! # Migration from TS extensions
//!
//! Each TS extension maps to a feature module:
//! - `pi.registerTool()` → `Feature::tools()` + `Feature::execute()`
//! - `pi.registerCommand()` → `Feature::commands()` + `Feature::handle_command()`
//! - `pi.on("event")` → `Feature::on_event()`
//! - `ctx.ui.notify()` → return `BusRequest::Notify` from `on_event()`
//! - Context injection → `Feature::provide_context()`

pub mod adapter;
pub mod auth;
pub mod auto_compact;
pub mod cleave;
pub mod clipboard;
pub mod context;
pub mod delegate;
pub mod harness_settings;
pub mod lifecycle;
pub mod manage_tools;
pub mod memory;
pub mod model_budget;
pub mod persona;
pub mod session_log;
pub mod terminal_title;
pub mod usage;
pub mod version_check;
