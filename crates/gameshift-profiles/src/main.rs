use gameshift_profiles::*;
use serde_json::json;
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!(
                "gameshift-profiles {} protocol 1",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        Some("daemon") => agent::run(),
        Some("status") | None => {
            println!(
                "{}",
                serde_json::to_string_pretty(&call(json!({"command":"get"}))?)
                    .map_err(|e| e.to_string())?
            );
            Ok(())
        }
        Some("auto") => {
            call(json!({"command":"automatic"}))?;
            Ok(())
        }
        Some("manual") if args.len() == 2 => {
            call(json!({"command":"manual","profile":args[1]}))?;
            Ok(())
        }
        Some("restore") => {
            call(json!({"command":"driver_automatic"}))?;
            Ok(())
        }
        Some("retry") => {
            call(json!({"command":"retry"}))?;
            Ok(())
        }
        _ => Err(
            "Usage : gameshift-profiles [status|auto|manual IDENTIFIANT|restore|retry|daemon]"
                .into(),
        ),
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
