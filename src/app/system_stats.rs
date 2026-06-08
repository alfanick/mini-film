use std::fmt::Write;
use sysinfo::{Pid, System};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceUsage {
    pub(crate) process_cpu_percent: f32,
    pub(crate) process_memory_bytes: u64,
    pub(crate) system_cpu_percent: f32,
    pub(crate) system_memory_used_bytes: u64,
    pub(crate) system_memory_total_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceUsageSummary {
    samples: u64,
    process_cpu_min: f32,
    process_cpu_max: f32,
    process_cpu_sum: f32,
    system_cpu_min: f32,
    system_cpu_max: f32,
    system_cpu_sum: f32,
    process_memory_min_bytes: u64,
    process_memory_max_bytes: u64,
    process_memory_sum_bytes: u64,
    system_memory_used_min_bytes: u64,
    system_memory_used_max_bytes: u64,
    system_memory_used_sum_bytes: u64,
    system_memory_total_min_bytes: u64,
    system_memory_total_max_bytes: u64,
}

impl Default for ResourceUsageSummary {
    fn default() -> Self {
        Self {
            samples: 0,
            process_cpu_min: f32::INFINITY,
            process_cpu_max: 0.0,
            process_cpu_sum: 0.0,
            system_cpu_min: f32::INFINITY,
            system_cpu_max: 0.0,
            system_cpu_sum: 0.0,
            process_memory_min_bytes: u64::MAX,
            process_memory_max_bytes: 0,
            process_memory_sum_bytes: 0,
            system_memory_used_min_bytes: u64::MAX,
            system_memory_used_max_bytes: 0,
            system_memory_used_sum_bytes: 0,
            system_memory_total_min_bytes: u64::MAX,
            system_memory_total_max_bytes: 0,
        }
    }
}

impl ResourceUsageSummary {
    pub(crate) fn add(&mut self, usage: &ResourceUsage) {
        self.samples += 1;
        self.process_cpu_min = self.process_cpu_min.min(usage.process_cpu_percent);
        self.process_cpu_max = self.process_cpu_max.max(usage.process_cpu_percent);
        self.process_cpu_sum += usage.process_cpu_percent;
        self.system_cpu_min = self.system_cpu_min.min(usage.system_cpu_percent);
        self.system_cpu_max = self.system_cpu_max.max(usage.system_cpu_percent);
        self.system_cpu_sum += usage.system_cpu_percent;

        self.process_memory_min_bytes = self
            .process_memory_min_bytes
            .min(usage.process_memory_bytes);
        self.process_memory_max_bytes = self
            .process_memory_max_bytes
            .max(usage.process_memory_bytes);
        self.process_memory_sum_bytes = self
            .process_memory_sum_bytes
            .saturating_add(usage.process_memory_bytes);

        self.system_memory_used_min_bytes = self
            .system_memory_used_min_bytes
            .min(usage.system_memory_used_bytes);
        self.system_memory_used_max_bytes = self
            .system_memory_used_max_bytes
            .max(usage.system_memory_used_bytes);
        self.system_memory_used_sum_bytes = self
            .system_memory_used_sum_bytes
            .saturating_add(usage.system_memory_used_bytes);

        self.system_memory_total_min_bytes = self
            .system_memory_total_min_bytes
            .min(usage.system_memory_total_bytes);
        self.system_memory_total_max_bytes = self
            .system_memory_total_max_bytes
            .max(usage.system_memory_total_bytes);
    }

    pub(crate) fn report_block(&self) -> String {
        if self.samples == 0 {
            return "Resource usage: unavailable\n".to_string();
        }

        let mut out = String::new();
        let samples = self.samples as f64;
        let process_cpu_avg = self.process_cpu_sum as f64 / samples;
        let system_cpu_avg = self.system_cpu_sum as f64 / samples;
        let process_memory_avg_mb = bytes_to_mb(self.process_memory_sum_bytes) / samples;
        let system_memory_used_avg_mb = bytes_to_mb(self.system_memory_used_sum_bytes) / samples;
        let system_memory_total_min_mb = bytes_to_mb(self.system_memory_total_min_bytes);
        let system_memory_total_max_mb = bytes_to_mb(self.system_memory_total_max_bytes);

        writeln!(out, "Resource usage ({} samples):", self.samples).ok();
        writeln!(out, "CPU usage:").ok();
        writeln!(
            out,
            "  process: min {:.1}% / max {:.1}% / avg {:.1}%",
            self.process_cpu_min, self.process_cpu_max, process_cpu_avg,
        )
        .ok();
        writeln!(
            out,
            "  system: min {:.1}% / max {:.1}% / avg {:.1}%",
            self.system_cpu_min, self.system_cpu_max, system_cpu_avg,
        )
        .ok();
        writeln!(out, "Memory:").ok();
        writeln!(
            out,
            "  process: min {:.2} MB / max {:.2} MB / avg {:.2} MB",
            bytes_to_mb(self.process_memory_min_bytes),
            bytes_to_mb(self.process_memory_max_bytes),
            process_memory_avg_mb,
        )
        .ok();
        writeln!(
            out,
            "  system: min {:.2} MB / max {:.2} MB / avg {:.2} MB | used of {:.2}..{:.2} MB total",
            bytes_to_mb(self.system_memory_used_min_bytes),
            bytes_to_mb(self.system_memory_used_max_bytes),
            system_memory_used_avg_mb,
            system_memory_total_min_mb,
            system_memory_total_max_mb,
        )
        .ok();
        out
    }
}

impl From<&ResourceUsage> for ResourceUsageSummary {
    fn from(value: &ResourceUsage) -> Self {
        let mut summary = ResourceUsageSummary::default();
        summary.add(value);
        summary
    }
}

pub(crate) fn sample_usage_block() -> Option<ResourceUsage> {
    let mut system = System::new_all();
    system.refresh_all();
    let pid = Pid::from(std::process::id() as usize);
    let process = system.process(pid)?;
    Some(ResourceUsage {
        process_cpu_percent: process.cpu_usage(),
        process_memory_bytes: process.memory(),
        system_cpu_percent: system.global_cpu_usage(),
        system_memory_used_bytes: system.used_memory(),
        system_memory_total_bytes: system.total_memory(),
    })
}

fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_usage_summary_collects_min_max_and_averages() {
        let mut summary = ResourceUsageSummary::default();
        summary.add(&ResourceUsage {
            process_cpu_percent: 5.0,
            process_memory_bytes: 1_024,
            system_cpu_percent: 4.0,
            system_memory_used_bytes: 8_192,
            system_memory_total_bytes: 16_384,
        });
        summary.add(&ResourceUsage {
            process_cpu_percent: 10.0,
            process_memory_bytes: 2_048,
            system_cpu_percent: 12.0,
            system_memory_used_bytes: 10_240,
            system_memory_total_bytes: 16_384,
        });

        let report = summary.report_block();
        assert!(report.contains("Resource usage (2 samples):"));
        assert!(report.contains("Resource usage"));
        assert!(report.contains("min 5.0%"));
        assert!(report.contains("max 10.0%"));
        assert!(report.contains("MB"));
    }
}
