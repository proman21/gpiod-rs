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

    /// Consumer string
    #[structopt(short, long, default_value = "gpioset")]
    consumer: String,

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
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid line offset"))?;
        let value = match v.trim() {
            "0" | "off" | "false" => false,
            "1" | "on" | "true" => true,
            _ => anyhow::bail!("Invalid line value"),
        };
        Ok(Self { line, value })
    }
}

#[paw::main]
fn main(args: Args) -> anyhow::Result<()> {
    if args.line_values.len() > gpiod::MAX_VALUES {
        anyhow::bail!("Too many lines");
    }

    let chip = gpiod::Chip::new(&args.chip)?;

    let (lines, mut values): (Vec<_>, Vec<_>) = args
        .line_values
        .into_iter()
        .map(|pair| (pair.line, pair.value))
        .unzip();

    let output = chip.request_lines(
        gpiod::Options::output(&lines)
            .active(args.active)
            .bias(args.bias)
            .drive(args.drive)
            .values(&values)
            .consumer(&args.consumer),
    )?;

    //output.set_values(values)?;
    output.get_values(&mut values)?;

    for value in values {
        print!("{} ", if value { 1 } else { 0 });
    }
    println!("");

    Ok(())
}
