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
/// `table_row_even`/`table_row_odd`) and merges `styles/default.css` on top, so
/// our rules win where they overlap and the defaults still fill the rest. The
/// merge is what keeps the framework's *adaptive* `table_row_odd` — the one
/// table semantic the templates use but rustloc does not define — so dropping
/// it would leave that tag unknown.
///
/// The registry keys the stylesheet by filename, so `default.css` is what
/// `get("default")` resolves. `.css` also outranks `.yaml` for the same base
/// name, which is why the legacy YAML is deleted rather than left beside it:
/// a stale copy would sit there looking authoritative while never loading.
///
/// # Errors
///
/// Fails if the embedded stylesheet has no `default` entry or the dispatch
/// config is rejected — both compile-time asset problems, not user input.
pub fn app() -> Result<App, anyhow::Error> {
    Ok(App::builder()
        .templates(embed_templates!("templates"))
        .theme(theme()?)
        .commands(crate::Commands::dispatch_config())?
        .build()?)
}

/// The theme the app renders with: `Theme::default()` with `styles/default.css`
/// merged over it.
///
/// Split out from [`app`] so a test can resolve the styles and assert what a tag
/// actually paints. That matters more than it looks: the CSS parser drops a
/// property it does not implement *silently*, leaving a style that resolves and
/// renders but carries no attributes — a failure only the emitted ANSI reveals.
/// Returning the theme is what lets `theme_carries_the_expected_attributes` look.
///
/// # Errors
///
/// Fails if the embedded stylesheet has no `default` entry — a compile-time
/// asset problem, not user input.
pub fn theme() -> Result<Theme, anyhow::Error> {
    let mut registry: StylesheetRegistry = embed_styles!("styles").into();
    Ok(Theme::default().merge(registry.get("default")?))
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
