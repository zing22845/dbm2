use std::collections::HashMap;

use dbm_core::Engine;

use crate::types::{DiscoveredInstance, DiscoveryCandidate, DiscoverySource, InstanceRunStatus};

#[derive(Debug, Clone)]
pub(crate) struct MergedCandidate {
    candidate: DiscoveryCandidate,
    sources: Vec<DiscoverySource>,
}

pub(crate) fn merge_candidates(candidates: Vec<DiscoveryCandidate>) -> Vec<MergedCandidate> {
    let mut merged: HashMap<String, MergedCandidate> = HashMap::new();

    for candidate in candidates {
        let key = candidate.fingerprint();
        merged
            .entry(key)
            .and_modify(|existing| {
                existing.sources.push(candidate.source);
                merge_fields(&mut existing.candidate, &candidate);
            })
            .or_insert(MergedCandidate {
                sources: vec![candidate.source],
                candidate,
            });
    }

    merge_by_endpoint(merged.into_values().collect())
}

fn merge_by_endpoint(entries: Vec<MergedCandidate>) -> Vec<MergedCandidate> {
    let mut merged: HashMap<(Engine, String, u16), MergedCandidate> = HashMap::new();

    for mut entry in entries {
        let key = (
            entry.candidate.engine,
            entry.candidate.host.clone(),
            entry.candidate.port,
        );
        merged
            .entry(key)
            .and_modify(|existing| {
                existing.sources.append(&mut entry.sources);
                merge_fields(&mut existing.candidate, &entry.candidate);
            })
            .or_insert(entry);
    }

    merged.into_values().collect()
}

fn merge_fields(existing: &mut DiscoveryCandidate, incoming: &DiscoveryCandidate) {
    if incoming.confidence > existing.confidence {
        existing.confidence = incoming.confidence;
    }
    if existing.data_dir.is_none() {
        existing.data_dir = incoming.data_dir.clone();
    }
    if existing.socket_path.is_none() {
        existing.socket_path = incoming.socket_path.clone();
    }
    if existing.systemd_unit.is_none() {
        existing.systemd_unit = incoming.systemd_unit.clone();
    }
    if existing.version.is_none() {
        existing.version = incoming.version.clone();
    }
    if incoming.status == InstanceRunStatus::Running {
        existing.status = incoming.status;
    }
}

pub(crate) fn finalize_instances(
    merged: Vec<MergedCandidate>,
    scanned_at: &str,
    registered: &HashMap<String, String>,
) -> Vec<DiscoveredInstance> {
    let mut instances: Vec<DiscoveredInstance> = merged
        .into_iter()
        .map(|entry| {
            let fingerprint = entry.candidate.fingerprint();
            let registered_instance_id = registered.get(&fingerprint).cloned();
            let mut sources = entry.sources;
            sources.sort_by_key(|s| source_rank(*s));
            sources.dedup();

            DiscoveredInstance {
                discovery_id: format!("dsc_{}", uuid::Uuid::new_v4().simple()),
                fingerprint,
                engine: entry.candidate.engine,
                host: entry.candidate.host,
                port: entry.candidate.port,
                socket_path: entry.candidate.socket_path,
                data_dir: entry.candidate.data_dir,
                systemd_unit: entry.candidate.systemd_unit,
                version: entry.candidate.version,
                status: entry.candidate.status,
                sources,
                confidence: entry.candidate.confidence,
                already_registered: registered_instance_id.is_some(),
                registered_instance_id,
                scanned_at: scanned_at.to_string(),
            }
        })
        .collect();

    instances.sort_by(|a, b| {
        (
            a.already_registered,
            a.host.as_str(),
            a.port,
            a.data_dir.as_deref().unwrap_or(""),
        )
            .cmp(&(
                b.already_registered,
                b.host.as_str(),
                b.port,
                b.data_dir.as_deref().unwrap_or(""),
            ))
    });
    instances
}

fn source_rank(source: DiscoverySource) -> u8 {
    match source {
        DiscoverySource::PidFile => 5,
        DiscoverySource::Process | DiscoverySource::Systemd => 4,
        DiscoverySource::Socket => 3,
        DiscoverySource::Port | DiscoverySource::Docker => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Confidence;

    #[test]
    fn merge_process_and_socket_on_same_endpoint() {
        let process = DiscoveryCandidate {
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5482,
            socket_path: None,
            data_dir: Some("/var/lib/postgresql/16".into()),
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            source: DiscoverySource::Process,
            confidence: Confidence::High,
        };
        let socket = DiscoveryCandidate {
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5482,
            socket_path: Some("/tmp/.s.PGSQL.5482".into()),
            data_dir: None,
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            source: DiscoverySource::Socket,
            confidence: Confidence::Medium,
        };

        let merged = merge_candidates(vec![process, socket]);
        assert_eq!(merged.len(), 1);
        let entry = &merged[0];
        assert_eq!(entry.candidate.port, 5482);
        assert_eq!(
            entry.candidate.data_dir.as_deref(),
            Some("/var/lib/postgresql/16")
        );
        assert_eq!(
            entry.candidate.socket_path.as_deref(),
            Some("/tmp/.s.PGSQL.5482")
        );
        assert!(entry.sources.contains(&DiscoverySource::Process));
        assert!(entry.sources.contains(&DiscoverySource::Socket));
    }
}
