use crate::metrics::Process;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub mode: i32,
    pub scale: i32,
    pub sort: i32,
    pub descending: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            mode: 3,
            scale: 90,
            sort: 2,
            descending: true,
        }
    }
}

impl Preferences {
    fn path() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("damned")
            .join("settings.json")
    }

    pub fn load() -> Self {
        let mut prefs: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        prefs.mode = prefs.mode.clamp(0, 3);
        prefs.scale = prefs.scale.clamp(70, 130);
        prefs.sort = prefs.sort.clamp(0, 5);
        prefs
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_vec(self)?)
    }
}

pub struct Group {
    pub key: String,
    pub name: String,
    pub children: Vec<Process>,
    pub cpu: Option<f64>,
    pub ram: Option<f64>,
    pub gpu: Option<f64>,
}

// Never present a partial group sum as a complete measurement.
fn total(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values.sum()
}

pub fn groups(processes: &[Process], prefs: &Preferences) -> Vec<Group> {
    let mut by_key: BTreeMap<String, Vec<Process>> = BTreeMap::new();
    for process in processes {
        by_key
            .entry(process.key.clone())
            .or_default()
            .push(process.clone());
    }
    let mut groups: Vec<Group> = by_key
        .into_iter()
        .map(|(key, mut children)| {
            children.sort_by_key(|p| p.pid);
            Group {
                key,
                name: children[0].name.clone(),
                cpu: total(children.iter().map(|p| p.cpu)),
                ram: total(children.iter().map(|p| p.ram)),
                gpu: total(children.iter().map(|p| p.gpu)),
                children,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        let metric = |g: &Group| match prefs.sort {
            1 => g.cpu,
            2 => g.ram,
            3 => g.gpu,
            _ => None,
        };
        let order = if prefs.sort == 0 {
            let order = a.name.to_lowercase().cmp(&b.name.to_lowercase());
            if prefs.descending {
                order.reverse()
            } else {
                order
            }
        } else {
            match (metric(a), metric(b)) {
                (Some(a), Some(b)) => {
                    if prefs.descending {
                        b.total_cmp(&a)
                    } else {
                        a.total_cmp(&b)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        };
        order.then_with(|| a.key.cmp(&b.key))
    });
    groups
}

pub struct AppState {
    pub prefs: Preferences,
    pub expanded: HashSet<String>,
    pub snapshot: crate::metrics::Snapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_sum_and_keep_unknown_values_unknown() {
        let processes = vec![
            Process {
                name: "a.exe".into(),
                key: "a".into(),
                pid: 1,
                cpu: Some(2.0),
                ram: Some(10.0),
                ..Default::default()
            },
            Process {
                name: "a.exe".into(),
                key: "a".into(),
                pid: 2,
                cpu: Some(3.0),
                ram: None,
                ..Default::default()
            },
        ];
        let grouped = groups(&processes, &Preferences::default());
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].children.len(), 2);
        assert_eq!(grouped[0].cpu, Some(5.0));
        assert_eq!(grouped[0].ram, None);
    }

    #[test]
    fn missing_metrics_sort_last_in_both_directions() {
        let processes = vec![
            Process {
                key: "unknown".into(),
                ..Default::default()
            },
            Process {
                key: "known".into(),
                ram: Some(10.0),
                ..Default::default()
            },
        ];
        for descending in [true, false] {
            let prefs = Preferences {
                descending,
                ..Default::default()
            };
            assert_eq!(groups(&processes, &prefs)[0].key, "known");
        }
    }
}
