#![cfg_attr(docsrs, feature(doc_cfg))]

mod asset;
mod bundle;
mod common;
mod dev;
mod fmt;
mod new;
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "topcoat", version)]
pub struct TopcoatCli {
    #[command(subcommand)]
    command: Command,
}

impl TopcoatCli {
    pub async fn run(self) {
        match self.command {
            Command::New(cmd) => cmd.run(),
            Command::Ui(cmd) => cmd.run(),
            Command::Fmt(cmd) => cmd.run().await,
            Command::Dev(cmd) => cmd.run().await,
            Command::Asset(cmd) => cmd.run().await,
            Command::Bundle(cmd) => cmd.run().await,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Create a new Topcoat application
    New(new::NewCommand),
    /// Start a development server
    Dev(dev::DevCommand),
    /// Format Topcoat macro bodies
    Fmt(fmt::FmtCommand),
    /// Inspect assets embedded in the binary
    Asset(asset::AssetCommand),
    /// Manage premade UI components in your project
    Ui(ui::UiCommand),
    /// Build a server, assets, and optional static launcher (release by default)
    Bundle(bundle::BundleCommand),
}

pub async fn run() {
    run_from(std::env::args_os()).await;
}

pub async fn run_from(args: impl IntoIterator<Item = std::ffi::OsString>) {
    common::version::warn_on_mismatch();
    TopcoatCli::parse_from(args).run().await;
}
