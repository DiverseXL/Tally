//! Scaffold: future entry point of the `tally` command line interface.

use clap::Parser;

#[derive(Parser)]
#[command(name = "tally", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
