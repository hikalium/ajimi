pub mod check;
pub mod fix;
pub mod repo;

use anyhow::Result;
use argh::FromArgs;

#[derive(FromArgs, PartialEq, Debug)]
/// markdown extension to include git commits
pub struct Args {
    #[argh(subcommand)]
    nested: SubArgs,
}
impl Args {
    pub fn run(&self) -> Result<()> {
        match &self.nested {
            SubArgs::Fix(args) => args.run(),
            SubArgs::Check(args) => args.run(),
        }
    }
}
#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
/// ajimi subcommands
pub enum SubArgs {
    Fix(crate::fix::Args),
    Check(crate::check::Args),
}
