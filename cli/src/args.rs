#[derive(clap::Parser)]
#[command(
    name = "gpio",
    version,
    about,
    propagate_version = true,
    // Command::trailing_var_ar is required to use ValueHint::CommandWithArguments
    trailing_var_arg = true,
)]
pub struct Args {
    /// GPIO commands
    #[clap(subcommand)]
    pub cmd: Cmd,
}

/*fn list_chips() -> Vec<String> {
    static mut CHIPS: Option<Vec<String>> = None;
    static INIT: std::sync::Once = std::sync::Once::new();

    unsafe {
        INIT.call_once(|| {
            CHIPS = Some(
                gpiod::Chip::list_devices()
                    .unwrap()
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
            );
        });
        CHIPS.unwrap()
    }
}*/

#[derive(clap::Parser)]
pub enum Cmd {
    /// List GPIO devices
    Detect,

    /// Get info about GPIO devices
    Info {
        /// GPIO chip paths or names (ex. gpiochip0)
        #[arg(value_parser)]
        chip: Vec<String>,
    },

    /// Get values from GPIO lines
    Get {
        /// Input bias
        #[arg(short, long, value_enum, default_value = "disable")]
        bias: gpiod::Bias,

        /// Active state
        #[arg(short, long, value_enum, default_value = "high")]
        active: gpiod::Active,

        /// Consumer string
        #[arg(short, long, value_parser, default_value = "gpioget")]
        consumer: String,

        /// GPIO chip path or name (ex. gpiochip0)
        #[arg(value_parser)]
        chip: std::path::PathBuf,

        /// GPIO lines (ex. 0 11)
        #[arg(value_parser, required = true, num_args = ..=gpiod::MAX_VALUES)]
        lines: Vec<gpiod::LineId>,
    },

    /// Set values into GPIO lines
    Set {
        /// Input bias
        #[arg(short, long, value_enum, default_value = "disable")]
        bias: gpiod::Bias,

        /// Active state
        #[arg(short, long, value_enum, default_value = "high")]
        active: gpiod::Active,

        /// Output drive
        #[arg(short, long, value_enum, default_value = "push-pull")]
        drive: gpiod::Drive,

        /// Consumer string
        #[arg(short, long, value_parser, default_value = "gpioset")]
        consumer: String,

        /// GPIO chip path or name (ex. gpiochip0)
        #[arg(value_parser)]
        chip: std::path::PathBuf,

        /// GPIO line-value pairs (ex. 0=1 11=0)
        #[arg(value_parser, required = true, num_args = ..=gpiod::MAX_VALUES)]
        line_values: Vec<LineValue>,
    },

    /// Monitor values on GPIO lines
    Mon {
        /// Input bias
        #[arg(short, long, value_enum, default_value = "disable")]
        bias: gpiod::Bias,

        /// Active state
        #[arg(short, long, value_enum, default_value = "high")]
        active: gpiod::Active,

        /// Edge to detect
        #[arg(short, long, value_enum, default_value = "both")]
        edge: gpiod::EdgeDetect,

        /// Consumer string
        #[arg(short, long, value_parser, default_value = "gpiomon")]
        consumer: String,

        /// GPIO chip path or name (ex. gpiochip0)
        #[arg(value_parser)]
        chip: std::path::PathBuf,

        /// GPIO lines (ex. 0 11)
        #[arg(value_parser, required = true, num_args = ..=gpiod::MAX_VALUES)]
        lines: Vec<gpiod::LineId>,
    },

    #[cfg(feature = "complete")]
    /// Generate autocompletion
    Complete {
        /// Shell name
        #[arg(short, long, value_enum, value_parser, default_value = "bash")]
        shell: clap_complete::Shell,
    },
}

#[derive(Clone)]
pub struct LineValue {
    pub line: gpiod::LineId,
    pub value: bool,
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
