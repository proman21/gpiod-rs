#[derive(structopt::StructOpt)]
struct Args {
    /// Input bias
    #[structopt(short, long, default_value = "disable")]
    bias: gpiod::Bias,

    /// Active state
    #[structopt(short, long, default_value = "high")]
    active: gpiod::Active,

    /// Edge to detect
    #[structopt(short, long, default_value = "both")]
    edge: gpiod::EdgeDetect,

    /// Consumer string
    #[structopt(short, long, default_value = "gpiomon")]
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
            .edge(args.edge)
            .bias(args.bias)
            .consumer(&args.consumer),
    )?;

    for event in input {
        let event = event?;
        println!(
            "line {}: {}-edge [{:?}]",
            args.lines[event.line as usize], event.edge, event.time,
        );
    }

    Ok(())
}
