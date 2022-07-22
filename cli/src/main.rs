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

        /// Consumer string
        #[structopt(short, long, default_value = "gpioget")]
        consumer: String,

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

        /// Consumer string
        #[structopt(short, long, default_value = "gpioset")]
        consumer: String,

        /// GPIO chip
        #[structopt()]
        chip: std::path::PathBuf,

        /// GPIO line-value pairs
        #[structopt()]
        line_values: Vec<LineValue>,
    },

    /// Monitor values on GPIO lines
    Mon {
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
        let value = match v.trim() {
            "0" | "off" | "false" => false,
            "1" | "on" | "true" => true,
            _ => anyhow::bail!("Invalid line value"),
        };
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

            for index in (0..chips.len()).rev() {
                let chip = &chips[index];
                println!("{}", chip);
                for line in 0..chip.num_lines() {
                    let line_info = chip.line_info(line).unwrap();
                    println!("\t line \t {}: \t {}", line, line_info);
                }
            }
        }

        Cmds::Get {
            bias,
            active,
            consumer,
            chip,
            lines,
        } => {
            if lines.len() > gpiod::MAX_VALUES {
                anyhow::bail!("Too many lines");
            }

            let chip = gpiod::Chip::new(&chip)?;

            let input = chip.request_lines(
                gpiod::Options::input(&lines)
                    .active(active)
                    .bias(bias)
                    .consumer(&consumer),
            )?;

            let values = lines.iter().map(|_| false).collect::<Vec<_>>();

            let values = input.get_values(values)?;

            for value in values {
                print!("{} ", if value { 1 } else { 0 });
            }
            println!("");
        }

        Cmds::Set {
            bias,
            active,
            drive,
            consumer,
            chip,
            line_values,
        } => {
            if line_values.len() > gpiod::MAX_VALUES {
                anyhow::bail!("Too many lines");
            }

            let chip = gpiod::Chip::new(&chip)?;

            let (lines, values): (Vec<_>, Vec<_>) = line_values
                .into_iter()
                .map(|pair| (pair.line, pair.value))
                .unzip();

            let output = chip.request_lines(
                gpiod::Options::output(&lines)
                    .active(active)
                    .bias(bias)
                    .drive(drive)
                    .values(&values)
                    .consumer(&consumer),
            )?;

            //output.set_values(values)?;
            let values = output.get_values(values)?;

            for value in values {
                print!("{} ", if value { 1 } else { 0 });
            }
            println!("");
        }

        Cmds::Mon {
            edge,
            bias,
            active,
            consumer,
            chip,
            lines,
        } => {
            if lines.len() > gpiod::MAX_VALUES {
                anyhow::bail!("Too many lines");
            }

            let chip = gpiod::Chip::new(&chip)?;

            let input = chip.request_lines(
                gpiod::Options::input(&lines)
                    .active(active)
                    .edge(edge)
                    .bias(bias)
                    .consumer(&consumer),
            )?;

            for event in input {
                let event = event?;
                println!(
                    "line {}: {}-edge [{:?}]",
                    lines[event.line as usize], event.edge, event.time,
                );
            }
        }
    }

    Ok(())
}
