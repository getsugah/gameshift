//! Diagnostic entry point for the CPU engine; the graphical workflow remains separate.
use gameshift_cpu::{
    runner::{run, Config},
    topology::Topology,
};
use std::io::{self, Read, Write};
fn main() {
    if let Err(error) = execute() {
        eprintln!("GameShift CPU : {error}");
        std::process::exit(1);
    }
}
fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let argument = std::env::args().nth(1);
    if argument.as_deref() == Some("--example-config") {
        println!(
            "{}",
            serde_json::to_string_pretty(&Config {
                duration: Some(gameshift_cpu::calibrated::CpuDuration::preset(
                    Default::default()
                )),
                ..Config::default()
            })?
        );
        return Ok(());
    }
    if argument.as_deref() == Some("--scaling-config") {
        let config = Config {
            workers: Topology::detect().scaling_steps(),
            duration: Some(gameshift_cpu::calibrated::CpuDuration::preset(
                Default::default(),
            )),
            ..Config::default()
        };
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }
    if argument.is_some() {
        return Err(
            "Options : --example-config, --scaling-config ; sinon configuration JSON sur stdin"
                .into(),
        );
    }
    let mut input = String::new();
    io::stdin().take(65537).read_to_string(&mut input)?;
    if input.len() > 65536 {
        return Err("Configuration trop volumineuse".into());
    }
    let config = serde_json::from_str(&input)?;
    let report = run(config).map_err(io::Error::other)?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    Ok(())
}
