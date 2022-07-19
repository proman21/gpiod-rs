#[derive(structopt::StructOpt)]
enum Cmds {
    /// List GPIO devices
    Detect,

    /// Get info about GPIO devices
    Info {
        /// GPIO chip paths
        #[structopt()]
        chip: Vec<String>,
    },

    /// Get values from GPIO lines
    Get {
        /// Input bias
        #[structopt(short, long, default_value = "disable")]
        bias: gpiod::Bias,

        /// Active state
        #[structopt(short, long, default_value = "high")]
        active: gpiod::Active,

        /// Request label
        #[structopt(short, long, default_value = "gpioset")]
        label: String,

        /// GPIO chip
        #[structopt()]
        chip: std::path::PathBuf,

        /// GPIO lines
        #[structopt()]
        lines: Vec<gpiod::LineId>,
    },

    /// Set values into GPIO lines
    Set {
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
    },
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
fn main(cmds: Cmds) -> anyhow::Result<()> {
    match cmds {
        Cmds::Detect => {
            let chips = gpiod::Chip::list_devices()?
                .into_iter()
                .map(gpiod::Chip::new)
                .collect::<std::io::Result<Vec<_>>>()?;

            chips
                .iter()
                .rev() //Do it in reverse order because the numbers of the GPIO chips go from high to low
                .for_each(|f| println!("{}", f));
        }

        Cmds::Info { chip } => {
            let chips = gpiod::Chip::list_devices()?
                .into_iter()
                .filter(|path| {
                    chip.is_empty()
                        || chip.iter().any(|chip| {
                            path.to_str()
                                .map(|path| path.ends_with(chip))
                                .unwrap_or(false)
                        })
                })
                .map(gpiod::Chip::new)
                .collect::<std::io::Result<Vec<_>>>()?;

            println!("Info for all {} GPIO chips", chips.len());

            for index in (0..chips.len()).rev() {
                let chip = &chips[index];
                println!("{}", chip);
                for line in 0..chip.num_lines() {
                    let line_info = chip.line_info(line).unwrap();
                    println!("\t Line \t {}: \t {}", line, line_info);
                }
            }
        }

        Cmds::Get {
            bias,
            active,
            label,
            chip,
            lines,
        } => {
            if lines.len() > gpiod::Values::MAX {
                anyhow::bail!("Too many lines");
            }

            let chip = gpiod::Chip::new(&chip)?;

            let input = chip.request_input(&lines, active, Default::default(), bias, &label)?;

            println!(
                "GPIO get {} offset {:?}. Values {}",
                chip,
                lines,
                input.get_values::<gpiod::Values>()?
            );
        }

        Cmds::Set {
            bias,
            active,
            drive,
            label,
            chip,
            line_values,
        } => {
            if line_values.len() > gpiod::Values::MAX {
                anyhow::bail!("Too many lines");
            }

            let chip = gpiod::Chip::new(&chip)?;

            let (lines, values): (Vec<_>, gpiod::Values) = line_values
                .into_iter()
                .map(|pair| (pair.line, pair.value))
                .unzip();

            let output = chip.request_output(
                &lines,
                active,
                Default::default(),
                bias,
                drive,
                Some(values),
                &label,
            )?;

            output.set_values(values)?;

            println!("GPIO get {} offset {:?}. Values {:?}", chip, lines, values);
        }
    }

    Ok(())
}
