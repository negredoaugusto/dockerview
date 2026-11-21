//! Converte o `ContainerStatsResponse` bruto em [`StatsSample`], seguindo as fórmulas do `docker stats`.

use bollard::models::ContainerStatsResponse;

use crate::model::StatsSample;

pub fn compute(raw: &ContainerStatsResponse) -> StatsSample {
    StatsSample {
        cpu_percent: cpu_percent(raw),
        mem_usage: mem_usage(raw),
        mem_limit: mem_limit(raw),
        mem_percent: {
            let limit = mem_limit(raw);
            if limit > 0 { mem_usage(raw) as f64 / limit as f64 * 100.0 } else { 0.0 }
        },
        net_rx: raw.networks.iter().flatten().filter_map(|(_, n)| n.rx_bytes).sum(),
        net_tx: raw.networks.iter().flatten().filter_map(|(_, n)| n.tx_bytes).sum(),
        pids: raw.pids_stats.as_ref().and_then(|p| p.current).unwrap_or(0),
    }
}

fn cpu_percent(raw: &ContainerStatsResponse) -> f64 {
    let (Some(cpu), Some(pre)) = (&raw.cpu_stats, &raw.precpu_stats) else {
        return 0.0;
    };
    let total = |s: &bollard::models::ContainerCpuStats| s.cpu_usage.as_ref().and_then(|u| u.total_usage).unwrap_or(0);
    let cpu_delta = total(cpu).saturating_sub(total(pre)) as f64;
    let system_delta = cpu.system_cpu_usage.unwrap_or(0).saturating_sub(pre.system_cpu_usage.unwrap_or(0)) as f64;

    let online = cpu
        .online_cpus
        .filter(|&n| n > 0)
        .map(f64::from)
        .or_else(|| cpu.cpu_usage.as_ref().and_then(|u| u.percpu_usage.as_ref()).map(|v| v.len() as f64))
        .unwrap_or(1.0);

    if system_delta > 0.0 && cpu_delta > 0.0 {
        cpu_delta / system_delta * online * 100.0
    } else {
        0.0
    }
}

/// Uso de memória descontando page cache (cgroup v2: `inactive_file`, v1: `total_inactive_file`).
fn mem_usage(raw: &ContainerStatsResponse) -> u64 {
    let Some(mem) = &raw.memory_stats else { return 0 };
    let usage = mem.usage.unwrap_or(0);
    let cache = mem
        .stats
        .as_ref()
        .and_then(|s| s.get("inactive_file").or_else(|| s.get("total_inactive_file")).copied())
        .unwrap_or(0);
    usage.saturating_sub(cache)
}

fn mem_limit(raw: &ContainerStatsResponse) -> u64 {
    raw.memory_stats.as_ref().and_then(|m| m.limit).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{ContainerCpuStats, ContainerCpuUsage, ContainerMemoryStats};
    use std::collections::HashMap;

    fn cpu(total: u64, system: u64) -> ContainerCpuStats {
        ContainerCpuStats {
            cpu_usage: Some(ContainerCpuUsage { total_usage: Some(total), ..Default::default() }),
            system_cpu_usage: Some(system),
            online_cpus: Some(4),
            ..Default::default()
        }
    }

    #[test]
    fn computes_cpu_and_memory() {
        let raw = ContainerStatsResponse {
            cpu_stats: Some(cpu(200, 2000)),
            precpu_stats: Some(cpu(100, 1000)),
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(600),
                limit: Some(1000),
                stats: Some(HashMap::from([("inactive_file".to_string(), 100)])),
                ..Default::default()
            }),
            ..Default::default()
        };
        let s = compute(&raw);
        assert!((s.cpu_percent - 40.0).abs() < 1e-9); // 100/1000 * 4 * 100
        assert_eq!(s.mem_usage, 500);
        assert!((s.mem_percent - 50.0).abs() < 1e-9);
    }

    #[test]
    fn empty_stats_are_zero() {
        assert_eq!(compute(&ContainerStatsResponse::default()), StatsSample::default());
    }
}
