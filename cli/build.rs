use std::io::Result;

#[cfg(feature = "genman")]
include!("src/args.rs");

fn main() -> Result<()> {
    #[cfg(feature = "genman")]
    genman()?;

    Ok(())
}

#[cfg(feature = "genman")]
fn genman() -> Result<()> {
    fn genpage(cmd: clap::Command, path: impl AsRef<std::path::Path>) -> Result<()> {
        let mut file = std::fs::File::create(path)?;
        let man = clap_mangen::Man::new(cmd);

        man.render(&mut file)?;

        Ok(())
    }

    let out_dir = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").ok_or_else(|| std::io::ErrorKind::NotFound)?,
    );

    let cmd = <Args as clap::CommandFactory>::command();

    genpage(cmd.clone(), out_dir.join(format!("{}.1", cmd.get_name())))?;

    for subcmd in cmd.get_subcommands() {
        genpage(
            subcmd.clone(),
            out_dir.join(format!("{}-{}.1", cmd.get_name(), subcmd.get_name())),
        )?;
    }

    Ok(())
}
