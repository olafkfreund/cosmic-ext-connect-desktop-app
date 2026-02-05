/// System information from remote device
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SystemInfo {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub total_memory: u64,
    pub used_memory: u64,
    pub disk_usage: f64,
    pub uptime: u64,
}
