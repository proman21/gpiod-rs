#[derive(structopt::StructOpt)]
struct Args {
    /// GPIO chip paths
    #[structopt()]
    chip: Vec<String>,
}

#[paw::main]
fn main(args: Args) -> anyhow::Result<()> {
    let chips = gpiod::Chip::list_devices()?
        .into_iter()
        .filter(|path| {
            args.chip.is_empty()
                || args.chip.iter().any(|chip| {
                    path.to_str()
                        .map(|path| path.ends_with(chip))
                        .unwrap_or(false)
                })
        })
        .map(gpiod::Chip::new)
        .collect::<std::io::Result<Vec<_>>>()?;

    for index in (0..chips.len()).rev() {
        let chip = &chips[index];
        println!("{}:", chip);
        for line in 0..chip.num_lines() {
            let line_info = chip.line_info(line).unwrap();
            println!("\t line \t {}: \t {}", line, line_info);
        }
    }

    Ok(())
}
