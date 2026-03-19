use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "acp-bus", version, about = "Multi-agent collaboration CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI interface
    Tui {
        /// Working directory
        #[arg(long, default_value = ".")]
        cwd: String,
    },
    /// Start JSON-RPC server over stdio
    Serve {
        /// Use stdio transport
        #[arg(long)]
        stdio: bool,
        /// Working directory
        #[arg(long, default_value = ".")]
        cwd: String,
    },
    /// List saved channel snapshots
    Channels {
        /// Working directory to search
        #[arg(long, default_value = ".")]
        cwd: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Tui { cwd } => {
            let cwd = std::fs::canonicalize(&cwd)?.to_string_lossy().to_string();

            // Setup terminal
            crossterm::terminal::enable_raw_mode()?;
            let mut stdout = std::io::stdout();
            crossterm::execute!(
                stdout,
                crossterm::terminal::EnterAlternateScreen,
                crossterm::event::EnableBracketedPaste,
            )?;
            let backend = ratatui::backend::CrosstermBackend::new(stdout);
            let mut terminal = ratatui::Terminal::new(backend)?;

            let mut app = acp_tui::App::new(cwd);
            let result = app.run(&mut terminal).await;

            // Restore terminal
            crossterm::terminal::disable_raw_mode()?;
            crossterm::execute!(
                terminal.backend_mut(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableBracketedPaste,
            )?;
            terminal.show_cursor()?;

            result?;
        }
        Commands::Serve { stdio, cwd } => {
            if !stdio {
                anyhow::bail!("only --stdio transport is supported");
            }
            let cwd = std::fs::canonicalize(&cwd)?.to_string_lossy().to_string();
            acp_server::serve_stdio(cwd).await?;
        }
        Commands::Channels { cwd } => {
            let cwd = std::fs::canonicalize(&cwd)?.to_string_lossy().to_string();
            let snapshots = acp_core::store::list_snapshots(&cwd).await?;
            if snapshots.is_empty() {
                println!("No saved channels found.");
            } else {
                for s in &snapshots {
                    println!(
                        "{} | {} | {} msgs | {}",
                        s.channel_id, s.saved_at, s.msg_count, s.agents
                    );
                }
            }
        }
    }

    Ok(())
}
