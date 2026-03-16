//! FerrisKey command-line client.

#![allow(clippy::print_stderr, clippy::print_stdout)]

#[tokio::main]
async fn main() -> eyre::Result<()> {
    match ferriskey_sdk::cli::parse_args(std::env::args_os()) {
        Ok(invocation) => {
            let output = ferriskey_sdk::cli::execute_with_transport(
                invocation,
                ferriskey_sdk::HpxTransport::default(),
            )
            .await?;
            println!("{output}");
            Ok(())
        }
        Err(ferriskey_sdk::cli::CliError::Clap(error)) => {
            if error.use_stderr() {
                eprint!("{error}");
                std::process::exit(2);
            }

            print!("{error}");
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
