use clap::{arg, Command};
use color_eyre::Report;

fn cli_args() -> Command {
    let add = Command::new("add")
        .about("add a torrent")
        .arg(arg!(<TORRENT_FILE> "the file to parse"))
        .arg_required_else_help(true);
    let list = Command::new("list").about("display current queue");

    Command::new("everlasting-client")
        .about("Bittorrent client")
        .arg_required_else_help(true)
        .subcommand_required(true)
        .subcommand(add)
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    let cmd = cli_args().get_matches();

    match cmd.subcommand() {
        Some(("add", sub_matches)) => {
            let args: Vec<_> = sub_matches
                .get_many::<String>("TORRENT_FILE")
                .expect("required!")
                .into_iter()
                .collect();
            println!("you tried to download {args:?}");
        }
        _ => unreachable!(),
    }

    Ok(())
}
