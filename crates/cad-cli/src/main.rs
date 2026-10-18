#![forbid(unsafe_code)]
use cad_access::{failure, Action, Config, Frontend, HELP};
use tokio::io::{AsyncWriteExt, BufReader};
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("cad-cli: {error}");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), String> {
    let config = match Config::parse()? {
        Action::Help => {
            print!("{HELP}");
            return Ok(());
        }
        Action::Version => {
            println!("cad-cli {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Action::Run(config) => config,
    };
    let one = config.command.clone();
    let mut frontend = Frontend::new(config);
    if let Some(command) = one {
        let output = match serde_json::from_str(&command) {
            Ok(command) => frontend.command(command).await,
            Err(e) => failure("invalid_request", e),
        };
        println!("{output}");
        return if output["ok"] == true {
            Ok(())
        } else {
            Err("command failed; see structured stdout response".into())
        };
    }
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(command) = cad_access::read_input(&mut input).await? {
        let reply = match command {
            Ok(command) => frontend.command(command).await,
            Err(e) => failure("invalid_request", e),
        };
        let mut bytes = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        output.write_all(&bytes).await.map_err(|e| e.to_string())?;
        output.flush().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
