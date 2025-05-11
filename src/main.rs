use anyhow::Result;

fn main() -> Result<()> {
    let args: ajimi::Args = argh::from_env();
    args.run()?;
    Ok(())
}
