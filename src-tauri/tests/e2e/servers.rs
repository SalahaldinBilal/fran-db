use std::{
    collections::HashMap,
    env,
    time::{Duration, Instant},
};

use fran_db_lib::connections::SecretBag;
use fran_db_lib::db::{DriverOptions, mysql::MysqlOptions, postgres::PostgresOptions, session::DatabaseSession};
use fran_db_lib::error::AppResult;
use fran_db_lib::ssh::known_hosts::KnownHosts;
use testcontainers::{Container, GenericImage, ImageExt, core::IntoContainerPort, runners::SyncRunner};

const PASSWORD: &str = "fran-e2e";
const LABEL: &str = "fran-db-e2e";
const READY_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Mysql,
    Postgres,
}

pub struct Spec {
    pub id: &'static str,
    pub flavor: Flavor,
    pub image: &'static str,
    pub tag: &'static str,
    pub product: &'static str,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "mysql_8_4",
        flavor: Flavor::Mysql,
        image: "mysql",
        tag: "8.4",
        product: "MySQL",
    },
    Spec {
        id: "postgres_17",
        flavor: Flavor::Postgres,
        image: "postgres",
        tag: "17",
        product: "PostgreSQL",
    },
];

impl Spec {
    /// Set to `host:port:user:password` to test against a running server instead of a container.
    pub fn external_variable(&self) -> String {
        format!("FRAN_E2E_{}", self.id.to_ascii_uppercase())
    }

    pub fn uses_container(&self) -> bool {
        env::var(self.external_variable()).is_err()
    }
}

pub struct Server {
    pub spec: &'static Spec,
    options: DriverOptions,
    password: String,
    known_hosts: KnownHosts,
    _container: Option<Container<GenericImage>>,
}

impl Server {
    pub async fn connect(&self) -> AppResult<Box<dyn DatabaseSession>> {
        let secrets = SecretBag::new(HashMap::from([("password".to_string(), self.password.clone())]));
        self.options.connect(&secrets, &self.known_hosts).await
    }
}

fn driver_options(flavor: Flavor, host: String, port: u16, user: String) -> DriverOptions {
    match flavor {
        Flavor::Mysql => DriverOptions::Mysql(MysqlOptions {
            host,
            port,
            user,
            ..Default::default()
        }),
        Flavor::Postgres => DriverOptions::Postgres(PostgresOptions {
            host,
            port,
            user,
            ..Default::default()
        }),
    }
}

fn known_hosts(spec: &Spec) -> KnownHosts {
    let path = env::temp_dir().join("fran-db-e2e").join(format!("{}-known_hosts.json", spec.id));
    KnownHosts::load(path)
}

fn external(spec: &'static Spec, value: &str) -> Result<Server, String> {
    let parts: Vec<&str> = value.splitn(4, ':').collect();
    let [host, port, user, password] = parts.as_slice() else {
        return Err(format!("{} should be host:port:user:password", spec.external_variable()));
    };
    let port: u16 = port
        .parse()
        .map_err(|_| format!("{} has an invalid port: {port}", spec.external_variable()))?;

    Ok(Server {
        spec,
        options: driver_options(spec.flavor, host.to_string(), port, user.to_string()),
        password: password.to_string(),
        known_hosts: known_hosts(spec),
        _container: None,
    })
}

fn container(spec: &'static Spec) -> Result<Server, String> {
    let (port, user, password_variable) = match spec.flavor {
        Flavor::Mysql => (3306, "root", "MYSQL_ROOT_PASSWORD"),
        Flavor::Postgres => (5432, "postgres", "POSTGRES_PASSWORD"),
    };
    let failed = |error: testcontainers::TestcontainersError| format!("{} didn't start: {error}", spec.id);

    let container = GenericImage::new(spec.image, spec.tag)
        .with_exposed_port(port.tcp())
        .with_label(LABEL, spec.id)
        .with_env_var(password_variable, PASSWORD)
        .start()
        .map_err(failed)?;

    let host = match container.get_host().map_err(failed)?.to_string() {
        host if host == "localhost" => "127.0.0.1".to_string(),
        host => host,
    };
    let mapped_port = container.get_host_port_ipv4(port).map_err(failed)?;

    Ok(Server {
        spec,
        options: driver_options(spec.flavor, host, mapped_port, user.to_string()),
        password: PASSWORD.to_string(),
        known_hosts: known_hosts(spec),
        _container: Some(container),
    })
}

/// Images restart the server after first-time setup, so this waits for a real login, not a log line.
async fn wait_until_ready(server: &Server) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match server.connect().await {
            Ok(session) => {
                session.close().await;
                return Ok(());
            }
            Err(error) if started.elapsed() > READY_TIMEOUT => {
                return Err(format!("{} never accepted connections: {error}", server.spec.id));
            }
            Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
        }
    }
}

pub fn start(spec: &'static Spec) -> Result<Server, String> {
    let server = match env::var(spec.external_variable()) {
        Ok(value) => external(spec, &value)?,
        Err(_) => container(spec)?,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(wait_until_ready(&server))?;
    Ok(server)
}
