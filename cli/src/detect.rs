#[paw::main]
fn main() -> anyhow::Result<()> {
    let chips = gpiod::Chip::list_devices()?
        .into_iter()
        .map(gpiod::Chip::new)
        .collect::<std::io::Result<Vec<_>>>()?;

    chips
        .iter()
        .rev() //Do it in reverse order because the numbers of the GPIO chips go from high to low
        .for_each(|f| println!("{}", f));

    Ok(())
}
