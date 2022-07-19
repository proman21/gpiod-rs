#[derive(structopt::StructOpt)]
struct Args {
    /// Input bias
    #[structopt(short, long, default_value = "disable")]
    bias: gpiod::Bias,

    /// Active state
    #[structopt(short, long, default_value = "high")]
    active: gpiod::Active,

    /// Output drive
    #[structopt(short, long, default_value = "push-pull")]
    drive: gpiod::Drive,

    /// Request label
    #[structopt(short, long, default_value = "gpioset")]
    label: String,

    /// GPIO chip
    #[structopt()]
    chip: std::path::PathBuf,

    /// GPIO line-value pairs
    #[structopt()]
    line_values: Vec<LineValue>,
}

struct LineValue {
    line: gpiod::LineId,
    value: bool,
}

impl std::str::FromStr for LineValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("Key-value pair expected (line=value)"))?;
        let line = k
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid line offset"))?;
        let value = v
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid line value"))?;
        Ok(Self { line, value })
    }
}

#[paw::main]
fn main(args: Args) -> anyhow::Result<()> {
    if args.line_values.len() > gpiod::Values::MAX {
        anyhow::bail!("Too many lines");
    }

    let chip = gpiod::Chip::new(&args.chip)?;

    let (lines, values): (Vec<_>, gpiod::Values) = args
        .line_values
        .into_iter()
        .map(|pair| (pair.line, pair.value))
        .unzip();

    let output = chip.request_output(
        &lines,
        args.active,
        Default::default(),
        args.bias,
        args.drive,
        Some(values),
        &args.label,
    )?;

    output.set_values(values)?;

    println!("GPIO get {} offset {:?}. Values {:?}", chip, lines, values);

    Ok(())
}
