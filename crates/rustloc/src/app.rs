//! Deterministic construction of the Standout app and the Clap command.
//!
//! `main` used to build both inline, which meant the only way to exercise the
//! wiring — theme merge, template registration, dispatch config, the injected
//! filter grid — was to spawn the real executable. Everything below is a pure
//! function of the compiled-in assets: same inputs, same app, no environment
//! read, no I/O. That is what lets a test build the identical app `main` runs
//! and drive it in-process.
//!
//! `main` keeps only what genuinely belongs to a process: reading
//! `std::env::args`, writing to stdout/stderr, and mapping the outcome to an
//! exit code.

use clap::{Command, CommandFactory};
use standout::cli::App;
use standout::{embed_styles, embed_templates, StylesheetRegistry, Theme};

/// Build the Standout app: templates, merged theme, and dispatch config.
///
/// The theme starts from Standout's defaults (which carry
/// `table_row_even`/`table_row_odd`) and merges our stylesheet on top, so our
/// rules win where they overlap and the defaults still fill the rest.
///
/// # Errors
///
/// Fails if the embedded stylesheet has no `default` entry or the dispatch
/// config is rejected — both compile-time asset problems, not user input.
pub fn app() -> Result<App, anyhow::Error> {
    let mut registry: StylesheetRegistry = embed_styles!("styles").into();
    let custom_theme = registry.get("default")?;
    let theme = Theme::default().merge(custom_theme);

    Ok(App::builder()
        .templates(embed_templates!("templates"))
        .theme(theme)
        .commands(crate::Commands::dispatch_config())?
        .build()?)
}

/// Build the Clap command: the derived grammar plus the injected
/// `--<field>-<op>` filter grid.
///
/// The injection is part of the grammar, not a `main` detail — a test that
/// built `Cli::command()` without it would be testing a CLI that does not
/// exist.
pub fn cli_command() -> Command {
    crate::filter_args::inject(crate::Cli::command())
}
