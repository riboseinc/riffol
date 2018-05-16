extern crate riffol;

use riffol::config::{get_config, Config};
use std::io;

fn main() -> io::Result<()> {
    let config: Config = get_config("riffol.conf")?;

    Ok(())
}