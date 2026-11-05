pub mod engine;
pub mod library_ui;
pub mod providers;
pub mod ui_commands;

// Slint generates unreachable embedding stubs for its native widgets. Keep the
// handwritten-code lint enabled and scope this allowance to generated code.
#[allow(clippy::todo)]
mod ui {
    slint::include_modules!();
}
pub use ui::*;
