use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use refreshagent::{
    config::{self, Config},
    runner, scan, service, ui,
};
use std::path::PathBuf;
#[derive(Parser)]
#[command(version, about = "Autonomous SEO using your local coding agent")]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    project: PathBuf,
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Optional managed Search Console data, using your account API key.
    Cloud {
        #[command(subcommand)]
        action: Cloud,
    },
    /// First-run TUI, or supply --yes and explicit configuration for headless setup.
    Init {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        site: Option<String>,
        #[arg(long)]
        roots: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        validation: Option<String>,
        #[arg(long)]
        delivery: Option<String>,
    },
    Doctor,
    Scan,
    Opportunities,
    Run {
        #[arg(long)]
        opportunity: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Run at most one eligible task if the persisted schedule is due.
    Tick,
    History,
    Config,
    Pause,
    Resume,
    /// Resolve reviewed/interrupted work. Does not merge, delete or reset changes.
    Recover {
        id: String,
        #[arg(long, default_value = "dismissed")]
        status: String,
    },
    Service {
        #[command(subcommand)]
        action: Service,
    },
}
#[derive(Subcommand)]
enum Service {
    Install {
        #[arg(long)]
        preview: bool,
    },
    Status,
    Uninstall,
}
#[derive(Subcommand)]
enum Cloud {
    Connect {
        #[arg(long)]
        property: String,
        #[arg(long)]
        mapping: PathBuf,
    },
    Sync,
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = config::repo_root(&cli.project)?;
    match cli.command {
        None => ui::dashboard(&root)?,
        Some(Commands::Cloud { action }) => match action {
            Cloud::Connect { property, mapping } => {
                refreshagent::cloud::connect(&root, &property, &mapping)?
            }
            Cloud::Sync => refreshagent::cloud::sync(&root)?,
        },
        Some(Commands::Init {
            yes,
            site,
            roots,
            agent,
            validation,
            delivery,
        }) => {
            if Config::path(&root).exists() {
                bail!("Configuration already exists; edit it with your preferred editor");
            }
            if !yes {
                ui::onboarding(&root)?;
            } else {
                let (mut c, _) = config::discover(&root);
                if let Some(v) = site {
                    c.site_url = v;
                }
                if let Some(v) = roots {
                    c.content_roots = v.split(',').map(|s| s.trim().into()).collect();
                }
                if let Some(v) = agent {
                    c.agent_executable = config::executable(&v).unwrap_or_else(|| v.clone().into());
                    c.agent = v;
                }
                if let Some(v) = validation {
                    c.validation = vec![v];
                }
                if let Some(v) = delivery {
                    c.delivery = v;
                }
                c.save(&root)?;
                println!("Saved {}", Config::path(&root).display());
            }
        }
        Some(Commands::Doctor) => {
            let (detected, framework) = config::discover(&root);
            let c = if Config::path(&root).exists() {
                Config::load(&root)?
            } else {
                detected
            };
            println!("Repository: {}\nFramework: {framework}\nAgent: {}\nContent: {} {:?}\nValidation: {:?}", root.display(), c.agent_executable.display(), c.content_mode, c.content_roots, c.validation);
            let status = std::process::Command::new(&c.agent_executable)
                .arg("--version")
                .status()?;
            if !status.success() {
                bail!("Agent version check failed");
            }
            println!("Agent installed. Authentication is checked by the agent when a run starts; no credentials are read by RefreshAgent.");
        }
        Some(Commands::Scan | Commands::Opportunities) => println!(
            "{}",
            serde_json::to_string_pretty(&scan::scan(&root, &Config::load(&root)?)?)?
        ),
        Some(Commands::Run {
            opportunity,
            dry_run,
        }) => {
            let c = Config::load(&root)?;
            if dry_run {
                let ops = scan::scan(&root, &c)?;
                let o = ops
                    .iter()
                    .find(|o| opportunity.as_ref().map(|id| *id == o.id).unwrap_or(true));
                if let Some(o) = o {
                    println!("{}", refreshagent::agent::prompt(&c, o));
                } else {
                    println!("No matching opportunity");
                }
            } else {
                println!(
                    "{}",
                    runner::run(&root, &c, opportunity.as_deref(), false, None)?
                );
            }
        }
        Some(Commands::Tick) => println!(
            "{}",
            runner::run(&root, &Config::load(&root)?, None, true, None)?
        ),
        Some(Commands::History) => println!(
            "{}",
            serde_json::to_string_pretty(&runner::history(&root)?)?
        ),
        Some(Commands::Config) => println!("{}", toml::to_string_pretty(&Config::load(&root)?)?),
        Some(Commands::Pause | Commands::Resume) => {
            let mut c = Config::load(&root)?;
            c.paused = matches!(cli.command, Some(Commands::Pause));
            c.save(&root)?;
        }
        Some(Commands::Recover { id, status }) => runner::resolve(&root, &id, &status)?,
        Some(Commands::Service { action }) => match action {
            Service::Install { preview } => service::install(&root, preview)?,
            Service::Status => service::manage(&root, "status")?,
            Service::Uninstall => service::manage(&root, "uninstall")?,
        },
    }
    Ok(())
}
