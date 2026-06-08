use std::fmt::Write;
use std::time::Duration;

use sysinfo::{Pid, System};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceUsage {
    pub(crate) process_cpu_percent: f32,
    pub(crate) process_memory_kib: u64,
    pub(crate) system_cpu_percent: f32,
    pub(crate) system_memory_used_kib: u64,
    pub(crate) system_memory_total_kib: u64,
    pub(crate) sample_age: Duration,
}
impl ResourceUsage {
    pub(crate) fn report_block(&self) -> String {
        let mut out = String::new();
        writeln!(out, "CPU usage:").ok();
        writeln!(
            out,
            "  process: {:.1}% (sampled over {:.1}s)",
            self.process_cpu_percent,
            self.sample_age.as_secs_f32()
        )
        .ok();
        writeln!(out, "  system: {:.1}%", self.system_cpu_percent).ok();
        writeln!(out, "Memory:").ok();
        writeln!(out, "  process: {} KiB", self.process_memory_kib).ok();
        writeln!(
            out,
            "  system: {} / {} KiB",
            self.system_memory_used_kib, self.system_memory_total_kib
        )
        .ok();
        out
    }
}

pub(crate) fn sample_usage_block() -> Option<ResourceUsage> {
    let mut system = System::new_all();
    system.refresh_all();
    let pid = Pid::from(std::process::id() as usize);
    let process = system.process(pid)?;
    Some(ResourceUsage {
        process_cpu_percent: process.cpu_usage(),
        process_memory_kib: process.memory(),
        system_cpu_percent: system.global_cpu_usage(),
        system_memory_used_kib: system.used_memory(),
        system_memory_total_kib: system.total_memory(),
        sample_age: Duration::from_secs(0),
    })
}
