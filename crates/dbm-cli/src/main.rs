use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dbm_core::{ConnectOpts, ConnectionPool, DatabaseDriver, Engine, SchemaIntrospector};
use dbm_discovery::{DiscoveryTarget, validate_host};
use dbm_driver_pg::PostgresDriver;
use dbm_store::{
    DiscoveredInstance, DiscoveryConfig, ManagedInstance, NewInstanceConnection, RegisterOptions,
    RunDiscoveryOptions, SessionResolveOptions, Store, UpdateInstanceConnection,
    format_connection_precheck, format_precheck_report, init_data_dir, parse_port_spec,
    resolve_sql_session,
};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "dbm", about = "Database management CLI", version)]
struct Cli {
    /// Directory for config.db and master.key (default: {executable_dir}/data)
    #[arg(long, env = "DBM_DATA_DIR", global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive terminal UI
    #[command(name = "interact", visible_alias = "i")]
    Interact {
        #[arg(long, env = "DBM_DATABASE_URL")]
        url: Option<String>,
        #[arg(long, short = 'i')]
        instance: Option<String>,
        #[arg(long, short = 'c')]
        connection: Option<String>,
        /// Extra hosts included in Discover scan (repeatable)
        #[arg(long)]
        discover_host: Vec<String>,
        /// Write debug-level logs to this file (off by default)
        #[arg(long, value_name = "PATH")]
        debug_log: Option<PathBuf>,
    },
    /// Verify connectivity and print server version
    Ping {
        #[arg(long, env = "DBM_DATABASE_URL")]
        url: Option<String>,
        #[arg(long, short = 'i')]
        instance: Option<String>,
        #[arg(long, short = 'c')]
        connection: Option<String>,
    },
    /// Run a single SQL statement
    Query {
        #[arg(long, env = "DBM_DATABASE_URL")]
        url: Option<String>,
        #[arg(long, short = 'i')]
        instance: Option<String>,
        #[arg(long, short = 'c')]
        connection: Option<String>,
        sql: String,
    },
    /// List tables in a schema
    Tables {
        #[arg(long, env = "DBM_DATABASE_URL")]
        url: Option<String>,
        #[arg(long, short = 'i')]
        instance: Option<String>,
        #[arg(long, short = 'c')]
        connection: Option<String>,
        #[arg(long, default_value = "public")]
        schema: String,
    },
    /// Scan for local PostgreSQL instances
    Discover {
        #[command(subcommand)]
        command: DiscoverCommands,
    },
    /// Manage registered database instances
    Instance {
        #[command(subcommand)]
        command: InstanceCommands,
    },
}

#[derive(Subcommand)]
enum DiscoverCommands {
    /// Run a discovery scan and cache results
    Scan {
        #[arg(long)]
        host: Vec<String>,
        /// Shared port spec for --host (TCP probes only). Loopback scans also run local discovery (process/pid/socket) and may report ports outside this list.
        #[arg(long, default_value = "5432,5433-5440")]
        ports: String,
        /// Per-host ports: HOST:PORTS (repeatable; IPv6 as [addr]:PORTS)
        #[arg(long)]
        target: Vec<String>,
        #[arg(long, default_value = "postgres")]
        engine: String,
        #[arg(long)]
        include_managed_hosts: bool,
    },
    /// List instances from the latest scan
    List {
        #[arg(long)]
        unregistered: bool,
    },
}

#[derive(Subcommand)]
enum InstanceCommands {
    /// List registered instances
    List,
    /// Run registration prechecks without registering
    Precheck {
        #[arg(required = true)]
        discovery_id: Vec<String>,
    },
    /// Register one or more discovered instances
    Register {
        #[arg(required = true)]
        discovery_id: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Remove a registered instance by name
    Unregister { name: String },
    /// Manage connections on a registered instance
    Connection {
        #[command(subcommand)]
        command: InstanceConnectionCommands,
    },
}

#[derive(Subcommand)]
enum InstanceConnectionCommands {
    /// List connections on an instance
    List {
        #[arg(long)]
        instance: String,
    },
    /// Test credentials without saving
    Test {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        user: String,
        #[arg(long, default_value = "postgres")]
        database: String,
        #[arg(long, env = "DBM_PASSWORD")]
        password: Option<String>,
        /// SSL mode: disable | allow | prefer | require | verify-ca | verify-full
        #[arg(long, default_value = "disable")]
        ssl_mode: String,
    },
    /// Add a connection (runs precheck before save)
    Add {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        user: String,
        #[arg(long, default_value = "postgres")]
        database: String,
        #[arg(long, env = "DBM_PASSWORD")]
        password: Option<String>,
        /// SSL mode: disable | allow | prefer | require | verify-ca | verify-full
        #[arg(long, default_value = "disable")]
        ssl_mode: String,
    },
    /// Remove a connection from an instance
    Remove {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        name: String,
    },
    /// Update an existing connection (runs precheck before save)
    Update {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        database: Option<String>,
        #[arg(long, env = "DBM_PASSWORD")]
        password: Option<String>,
        #[arg(long)]
        clear_password: bool,
        #[arg(long)]
        ssl_mode: Option<String>,
        #[arg(long)]
        env_label: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_cli_tracing(&cli);
    if let Err(err) = dispatch(cli) {
        eprintln!("error: {err:#}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Initialize the tracing subscriber for non-interactive commands.
///
/// The interactive TUI owns its own subscriber: when `interact --debug-log
/// <path>` is given, `dbm-tui2` writes `debug` logs to that file; otherwise the
/// TUI installs the default `warn`/stderr subscriber. Other commands use the
/// `RUST_LOG`-filtered stderr subscriber here.
fn init_cli_tracing(cli: &Cli) {
    // When `interact --debug-log <path>` is given, the TUI installs the file
    // subscriber; skip the CLI one so it can `try_init` without conflicting.
    if matches!(
        &cli.command,
        Commands::Interact {
            debug_log: Some(_),
            ..
        }
    ) {
        return;
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();
}

fn dispatch(cli: Cli) -> anyhow::Result<()> {
    init_data_dir(cli.data_dir.clone()).map_err(anyhow::Error::from)?;
    match cli.command {
        // The interactive TUI (dbm-tui2) blocks on its own tokio runtime.
        Commands::Interact {
            url,
            instance,
            connection,
            discover_host: _,
            debug_log,
        } => run_interact(url, instance, connection, debug_log),
        other => tokio_run(Cli {
            data_dir: cli.data_dir,
            command: other,
        }),
    }
}

/// Run the interactive TUI. The CLI may pass `url`/`instance`/`connection` to
/// pre-select a session; `dbm-tui2` reads its connections from the store.
/// `debug_log` (when set) captures `debug`-level logs to that file.
fn run_interact(
    url: Option<String>,
    instance: Option<String>,
    connection: Option<String>,
    debug_log: Option<PathBuf>,
) -> anyhow::Result<()> {
    let _ = (url, instance, connection); // session pre-selection not yet plumbed
    dbm_tui2::app::run_with_log_file(debug_log)
}

#[tokio::main]
async fn tokio_run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Interact { .. } => unreachable!("interact handled in dispatch"),
        Commands::Ping {
            url,
            instance,
            connection,
        } => cmd_ping(url, instance, connection).await?,
        Commands::Query {
            url,
            instance,
            connection,
            sql,
        } => cmd_query(url, instance, connection, &sql).await?,
        Commands::Tables {
            url,
            instance,
            connection,
            schema,
        } => cmd_tables(url, instance, connection, &schema).await?,
        Commands::Discover { command } => cmd_discover(command)?,
        Commands::Instance { command } => cmd_instance(command)?,
    }
    Ok(())
}

async fn cmd_ping(
    url: Option<String>,
    instance: Option<String>,
    connection: Option<String>,
) -> anyhow::Result<()> {
    let pool = connect(url, instance, connection).await?;
    let driver = PostgresDriver;
    let version = driver.ping(&pool).await?;
    println!("ok");
    println!("{version}");
    Ok(())
}

async fn cmd_query(
    url: Option<String>,
    instance: Option<String>,
    connection: Option<String>,
    sql: &str,
) -> anyhow::Result<()> {
    let pool = connect(url, instance, connection).await?;
    let driver = PostgresDriver;
    let result = driver.execute_query(&pool, sql).await?;
    if let Some(affected) = result.rows_affected
        && result.columns.is_empty()
    {
        println!("{affected} row(s) affected");
        return Ok(());
    }
    print_table(&result);
    Ok(())
}

async fn cmd_tables(
    url: Option<String>,
    instance: Option<String>,
    connection: Option<String>,
    schema: &str,
) -> anyhow::Result<()> {
    let pool = connect(url, instance, connection).await?;
    let driver = PostgresDriver;
    let tables = driver.list_tables(&pool, schema).await?;
    if tables.is_empty() {
        println!("(no tables in schema `{schema}`)");
    } else {
        for table in tables {
            println!("{table}");
        }
    }
    Ok(())
}

fn cmd_discover(command: DiscoverCommands) -> anyhow::Result<()> {
    let store = Store::open_default()?;
    match command {
        DiscoverCommands::Scan {
            host,
            ports,
            target,
            engine,
            include_managed_hosts,
        } => {
            let config = build_discovery_config(&engine, host, &ports, target)?;
            let result = store.run_discovery_with_options(
                config,
                RunDiscoveryOptions {
                    include_managed_hosts,
                },
            )?;
            println!(
                "scan {} complete: {} instance(s)",
                result.scan_id,
                result.instances.len()
            );
            for item in &result.instances {
                print_discovered(item);
            }
        }
        DiscoverCommands::List { unregistered } => {
            let items = store.list_discovered(unregistered)?;
            if items.is_empty() {
                println!("(no discovered instances — run `dbm discover scan` first)");
                return Ok(());
            }
            for item in items {
                print_discovered(&item);
            }
        }
    }
    Ok(())
}

fn build_discovery_config(
    engine: &str,
    host: Vec<String>,
    ports: &str,
    target: Vec<String>,
) -> anyhow::Result<DiscoveryConfig> {
    let engine = match engine {
        "postgres" => Engine::Postgres,
        other => anyhow::bail!("unknown engine `{other}`"),
    };

    let mut config = DiscoveryConfig {
        engine,
        ..Default::default()
    };

    if !target.is_empty() {
        config.hosts.clear();
        config.targets = target
            .into_iter()
            .map(|raw| {
                let (host, ports) =
                    dbm_discovery::split_host_ports(&raw).map_err(anyhow::Error::msg)?;
                Ok(DiscoveryTarget {
                    host: validate_host(host).map_err(anyhow::Error::msg)?,
                    ports: parse_port_spec(ports).map_err(anyhow::Error::msg)?,
                })
            })
            .collect::<anyhow::Result<_>>()?;
        return Ok(config);
    }

    let hosts = if host.is_empty() {
        vec!["127.0.0.1".to_string()]
    } else {
        host
    };
    config.hosts = hosts
        .into_iter()
        .map(|raw| validate_host(&raw).map_err(anyhow::Error::msg))
        .collect::<anyhow::Result<_>>()?;
    config.ports = parse_port_spec(ports).map_err(anyhow::Error::msg)?;
    Ok(config)
}

fn cmd_instance(command: InstanceCommands) -> anyhow::Result<()> {
    let store = Store::open_default()?;
    match command {
        InstanceCommands::List => {
            let items = store.list_managed_instances()?;
            if items.is_empty() {
                println!("(no registered instances)");
                return Ok(());
            }
            for item in items {
                print_managed(&item, &store)?;
            }
        }
        InstanceCommands::Precheck { discovery_id } => {
            let options = RegisterOptions::default();
            let checks = store.precheck_register(&discovery_id, &options)?;
            print_prechecks(&checks);
            if checks.iter().all(|c| c.ok_to_register(false)) {
                println!("precheck: ok to register");
            } else if checks.iter().all(|c| c.ok_to_register(true)) {
                println!("precheck: warnings only — use `--force` to register");
            } else {
                println!("precheck: blocked by errors");
            }
        }
        InstanceCommands::Register {
            discovery_id,
            name,
            force,
        } => {
            let options = RegisterOptions { name, force };
            let result = store.register_discovered(&discovery_id, options)?;
            for item in &result.instances {
                println!("registered `{}` ({})", item.name, item.display_target());
            }
            let warnings: Vec<_> = result
                .prechecks
                .iter()
                .flat_map(|c| c.issues.iter())
                .filter(|i| i.level == dbm_store::PrecheckLevel::Warning)
                .collect();
            if !warnings.is_empty() {
                println!("warnings during registration:");
                for issue in warnings {
                    println!(
                        "  [{}] {}: {}",
                        issue.level.as_str(),
                        issue.code,
                        issue.message
                    );
                }
            }
        }
        InstanceCommands::Unregister { name } => {
            if store.unregister_managed(&name)? {
                println!("unregistered `{name}`");
            } else {
                anyhow::bail!("instance `{name}` not found");
            }
        }
        InstanceCommands::Connection { command } => cmd_instance_connection(&store, command)?,
    }
    Ok(())
}

fn cmd_instance_connection(
    store: &Store,
    command: InstanceConnectionCommands,
) -> anyhow::Result<()> {
    let ping_url = |url: &str| {
        let url = url.to_string();
        async move {
            let opts = ConnectOpts::new(url);
            let driver = PostgresDriver;
            let pool = driver.connect(&opts).await.map_err(|e| e.to_string())?;
            driver.ping(&pool).await.map_err(|e| e.to_string())
        }
    };

    match command {
        InstanceConnectionCommands::List { instance } => {
            let conns = store.list_instance_connections(&instance)?;
            if conns.is_empty() {
                println!("(no connections on `{instance}`)");
                return Ok(());
            }
            let inst = store.get_managed_instance_by_name(&instance)?;
            for c in conns {
                let pwd = if c.has_password { "pwd" } else { "no-pwd" };
                println!("  {:<12} {} [{pwd}]", c.name, c.display_target(&inst));
            }
        }
        InstanceConnectionCommands::Test {
            instance,
            name,
            user,
            database,
            password,
            ssl_mode,
        } => {
            let input = new_connection_input(name, user, database, password, ssl_mode);
            let precheck = store.test_instance_connection(&instance, &input, ping_url)?;
            println!(
                "{}",
                format_connection_precheck(&instance, &input.name, &precheck)
            );
            if precheck.ok {
                println!("test: ok");
            } else {
                println!("test: failed");
            }
        }
        InstanceConnectionCommands::Add {
            instance,
            name,
            user,
            database,
            password,
            ssl_mode,
        } => {
            let input = new_connection_input(name, user, database, password, ssl_mode);
            let conn = store.add_instance_connection(&instance, input, ping_url)?;
            let inst = store.get_managed_instance_by_name(&instance)?;
            println!(
                "added `{}` on `{}` ({})",
                conn.name,
                instance,
                conn.display_target(&inst)
            );
        }
        InstanceConnectionCommands::Remove { instance, name } => {
            if store.delete_instance_connection(&instance, &name)? {
                println!("removed `{name}` from `{instance}`");
            } else {
                anyhow::bail!("connection `{name}` not found on `{instance}`");
            }
        }
        InstanceConnectionCommands::Update {
            instance,
            name,
            user,
            database,
            password,
            clear_password,
            ssl_mode,
            env_label,
        } => {
            if user.is_none()
                && database.is_none()
                && password.is_none()
                && !clear_password
                && ssl_mode.is_none()
                && env_label.is_none()
            {
                anyhow::bail!(
                    "at least one of --user, --database, --password, --clear-password, --ssl-mode, --env-label is required"
                );
            }
            if clear_password && password.is_some() {
                anyhow::bail!("--clear-password cannot be combined with --password");
            }
            let patch = UpdateInstanceConnection {
                name: None,
                username: user,
                database,
                password: if clear_password {
                    Some(None)
                } else if password.is_some() {
                    Some(password)
                } else {
                    None
                },
                ssl_mode,
                env_label,
            };
            let conn = store.update_instance_connection(&instance, &name, patch, ping_url)?;
            let inst = store.get_managed_instance_by_name(&instance)?;
            println!(
                "updated `{}` on `{}` ({})",
                conn.name,
                instance,
                conn.display_target(&inst)
            );
        }
    }
    Ok(())
}

fn new_connection_input(
    name: String,
    user: String,
    database: String,
    password: Option<String>,
    ssl_mode: String,
) -> NewInstanceConnection {
    NewInstanceConnection {
        name,
        username: user,
        database,
        password,
        ssl_mode: Some(ssl_mode),
        env_label: None,
    }
}

fn print_discovered(item: &DiscoveredInstance) {
    let reg = if item.already_registered {
        "registered"
    } else {
        "new"
    };
    let sources: Vec<_> = item.sources.iter().map(|s| s.as_str()).collect();
    println!(
        "{reg:<11} {}  {}:{}  conf={}  status={}  sources=[{}]",
        item.discovery_id,
        item.host,
        item.port,
        item.confidence.as_str(),
        item.status.as_str(),
        sources.join(","),
    );
    if let Some(dir) = &item.data_dir {
        println!("           data_dir={dir}");
    }
}

fn print_managed(item: &ManagedInstance, store: &Store) -> anyhow::Result<()> {
    println!("{:<16} {}  {}", item.name, item.id, item.display_target());
    if let Some(v) = &item.version_short {
        println!("                 version={v}");
    }
    if let Some(full) = &item.version_full {
        println!("                 version_full={full}");
    }
    if let Some(status) = &item.lifecycle_status {
        print!("                 lifecycle={status}");
        if let Some(detail) = &item.lifecycle_detail {
            print!(" ({detail})");
        }
        println!();
    }
    if let Some(dir) = &item.data_dir {
        println!("                 data_dir={dir}");
    }
    let conns = store.list_instance_connections(&item.name)?;
    if conns.is_empty() {
        println!("                 connections: (none — add with `dbm instance connection add`)");
    } else {
        for c in conns {
            println!("                   {} → {}", c.name, c.display_target(item));
        }
    }
    Ok(())
}

fn print_prechecks(checks: &[dbm_store::InstancePrecheck]) {
    println!("{}", format_precheck_report(checks));
}

async fn connect(
    url: Option<String>,
    instance: Option<String>,
    connection: Option<String>,
) -> anyhow::Result<ConnectionPool> {
    let session = resolve_sql_session(SessionResolveOptions {
        url,
        instance,
        connection,
    })?;
    let opts = ConnectOpts::new(session.url);
    let driver = PostgresDriver;
    driver.connect(&opts).await.map_err(Into::into)
}

fn print_table(result: &dbm_core::QueryResult) {
    if result.columns.is_empty() {
        return;
    }
    let headers: Vec<&str> = result.columns.iter().map(|c| c.name.as_str()).collect();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    let mut rendered: Vec<Vec<String>> = Vec::new();

    for row in &result.rows {
        let line: Vec<String> = row
            .values
            .iter()
            .enumerate()
            .map(|(idx, value)| {
                widths[idx] = widths[idx].max(value.len());
                value.clone()
            })
            .collect();
        rendered.push(line);
    }

    print_row(
        &headers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &widths,
    );
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    print_row(&sep, &widths);
    for line in rendered {
        print_row(&line, &widths);
    }
}

fn print_row(cells: &[String], widths: &[usize]) {
    let parts: Vec<String> = cells
        .iter()
        .enumerate()
        .map(|(idx, cell)| format!("{cell:<width$}", width = widths[idx]))
        .collect();
    println!("{}", parts.join("  "));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_config_defaults_to_loopback_postgres() {
        let config = build_discovery_config("postgres", vec![], "5432", vec![]).unwrap();

        assert_eq!(config.hosts, vec!["127.0.0.1"]);
        assert!(config.targets.is_empty());
        assert_eq!(config.ports, vec![5432]);
        assert_eq!(config.engine, dbm_core::Engine::Postgres);
    }

    #[test]
    fn scan_config_parses_per_host_targets() {
        let config = build_discovery_config(
            "postgres",
            vec!["ignored.example.com".into()],
            "9999",
            vec!["127.0.0.1:5432,5433-5434".into(), "[::1]:5440".into()],
        )
        .unwrap();

        assert!(config.hosts.is_empty());
        assert_eq!(
            config.targets,
            vec![
                dbm_discovery::DiscoveryTarget {
                    host: "127.0.0.1".into(),
                    ports: vec![5432, 5433, 5434],
                },
                dbm_discovery::DiscoveryTarget {
                    host: "::1".into(),
                    ports: vec![5440],
                },
            ]
        );
    }

    #[test]
    fn scan_config_rejects_unknown_engine() {
        let err = build_discovery_config("mysql", vec![], "5432", vec![]).unwrap_err();

        assert!(err.to_string().contains("unknown engine `mysql`"));
    }

    #[test]
    fn scan_config_rejects_invalid_target_syntax() {
        let err = build_discovery_config("postgres", vec![], "5432", vec!["localhost".into()])
            .unwrap_err();

        assert!(err.to_string().contains("HOST:PORTS"));
    }
}
