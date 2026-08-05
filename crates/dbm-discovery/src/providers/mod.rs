mod pidfile;
mod port;
mod process;
mod socket;

use crate::context::DiscoveryConfig;
use crate::types::DiscoveryCandidate;

pub(crate) use port::PortProvider;

pub trait DiscoveryProvider {
    fn discover(&self, config: &DiscoveryConfig) -> Vec<DiscoveryCandidate>;
}

pub fn local_postgres_providers() -> Vec<Box<dyn DiscoveryProvider>> {
    vec![
        Box::new(pidfile::PidFileProvider),
        Box::new(process::ProcessProvider),
        Box::new(socket::SocketProvider),
    ]
}
