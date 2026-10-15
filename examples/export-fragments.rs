//! Export a completed snapshot for an existing site's build.

use std::path::PathBuf;

use clap::Parser;
use diplodocus::{rendering::render_fragments, site::Site, snapshots::Snapshot};

#[derive(Parser)]
struct Arguments {
    /// Completed SQLite documentation snapshot.
    #[arg(long)]
    input: PathBuf,
    /// Directory for document fragments, metadata, and assets.
    #[arg(long)]
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arguments::parse();
    let snapshot = Snapshot::load(args.input)?;
    render_fragments(&Site::new(&snapshot)?)?.publish(&args.output)?;
    Ok(())
}
