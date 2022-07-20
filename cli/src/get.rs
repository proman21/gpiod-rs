#[derive(structopt::StructOpt)]
struct Args {
    /// Input bias
    #[structopt(short, long, default_value = "disable")]
    bias: gpiod::Bias,

    /// Active state
    #[structopt(short, long, default_value = "high")]
    active: gpiod::Active,

    /// Consumer string
    #[structopt(short, long, default_value = "gpioget")]
    consumer: String,

    /// GPIO chip
    #[structopt()]
    chip: std::path::PathBuf,

    /// GPIO lines
    #[structopt()]
    lines: Vec<gpiod::LineId>,
}

#[paw::main]
fn main(args: Args) -> anyhow::Result<()> {
    if args.lines.len() > gpiod::Values::MAX {
        anyhow::bail!("Too many lines");
    }

    let chip = gpiod::Chip::new(&args.chip)?;

    let input = chip.request_lines(
        gpiod::Options::input(&args.lines)
            .active(args.active)
            .bias(args.bias)
            .consumer(&args.consumer),
    )?;

    for value in input.get_values::<gpiod::Values>()? {
        print!("{}", if value { 1 } else { 0 });
    }
    println!("");

    Ok(())
}
