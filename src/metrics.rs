use std::collections::HashMap;
use std::mem::{size_of, zeroed};
use std::time::Instant;
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE},
    NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Performance::*,
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        SystemInformation::{GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO},
        Threading::{
            GetProcessTimes, GetSystemTimes, OpenProcess, QueryFullProcessImageNameW,
            PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
        },
    },
};

#[derive(Clone, Debug, Default)]
pub struct Process {
    pub pid: u32,
    pub identity: u64,
    pub name: String,
    pub key: String,
    pub cpu: Option<f64>,
    pub ram: Option<f64>,
    pub gpu: Option<f64>,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub cpu: Option<f64>,
    pub ram_percent: Option<f64>,
    pub ram_used: Option<f64>,
    pub ram_total: Option<f64>,
    pub gpu: Option<f64>,
    pub vram_used: Option<f64>,
    pub download: Option<f64>,
    pub upload: Option<f64>,
    pub read: Option<f64>,
    pub write: Option<f64>,
    pub processes: Vec<Process>,
}

pub fn delta_rate(previous: u64, current: u64, seconds: f64) -> Option<f64> {
    (seconds > 0.0)
        .then(|| current.checked_sub(previous).map(|d| d as f64 / seconds))
        .flatten()
}

fn ticks(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

// This object and all PDH handles stay on the sampling thread.
struct Counters {
    query: PDH_HQUERY,
    gpu: PDH_HCOUNTER,
    vram: PDH_HCOUNTER,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
}

impl Counters {
    fn new() -> Self {
        unsafe {
            let mut result = Self {
                query: std::ptr::null_mut(),
                gpu: std::ptr::null_mut(),
                vram: std::ptr::null_mut(),
                read: std::ptr::null_mut(),
                write: std::ptr::null_mut(),
            };
            if PdhOpenQueryW(std::ptr::null(), 0, &mut result.query) == 0 {
                result.gpu = result.add(r"\GPU Engine(*)\Utilization Percentage");
                result.vram = result.add(r"\GPU Adapter Memory(*)\Dedicated Usage");
                result.read = result.add(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec");
                result.write = result.add(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec");
            }
            result
        }
    }

    unsafe fn add(&self, path: &str) -> PDH_HCOUNTER {
        let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut handle = std::ptr::null_mut();
        if PdhAddEnglishCounterW(self.query, path.as_ptr(), 0, &mut handle) == 0 {
            handle
        } else {
            std::ptr::null_mut()
        }
    }

    fn collect(&self) -> bool {
        !self.query.is_null() && unsafe { PdhCollectQueryData(self.query) == 0 }
    }

    fn scalar(handle: PDH_HCOUNTER) -> Option<f64> {
        if handle.is_null() {
            return None;
        }
        unsafe {
            let mut value: PDH_FMT_COUNTERVALUE = zeroed();
            if PdhGetFormattedCounterValue(handle, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut value)
                != 0
                || !matches!(value.CStatus, 0 | 1)
            {
                return None;
            }
            let n = value.Anonymous.doubleValue;
            (n.is_finite() && n >= 0.0).then_some(n)
        }
    }

    fn array(handle: PDH_HCOUNTER) -> Option<Vec<(String, f64)>> {
        if handle.is_null() {
            return None;
        }
        unsafe {
            let mut bytes = 0;
            let mut count = 0;
            PdhGetFormattedCounterArrayW(
                handle,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                std::ptr::null_mut(),
            );
            if bytes == 0 {
                return None;
            }
            // u64 backing guarantees alignment for the native array.
            let mut storage = vec![0u64; (bytes as usize).div_ceil(8)];
            let items = storage.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if PdhGetFormattedCounterArrayW(handle, PDH_FMT_DOUBLE, &mut bytes, &mut count, items)
                != 0
            {
                return None;
            }
            let mut values = Vec::new();
            for item in std::slice::from_raw_parts(items, count as usize) {
                if !matches!(item.FmtValue.CStatus, 0 | 1) || item.szName.is_null() {
                    continue;
                }
                let mut len = 0;
                while *item.szName.add(len) != 0 {
                    len += 1;
                }
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(item.szName, len));
                let value = item.FmtValue.Anonymous.doubleValue;
                if value.is_finite() && value >= 0.0 {
                    values.push((name, value));
                }
            }
            (!values.is_empty()).then_some(values)
        }
    }
}

impl Drop for Counters {
    fn drop(&mut self) {
        if !self.query.is_null() {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }
}

pub struct Collector {
    counters: Counters,
    last_time: Instant,
    last_system: Option<(u64, u64)>,
    last_process: HashMap<(u32, u64), u64>,
    last_network: HashMap<u64, (u64, u64)>,
    processors: f64,
}

impl Collector {
    pub fn new() -> Self {
        let mut info: SYSTEM_INFO = unsafe { zeroed() };
        unsafe {
            GetSystemInfo(&mut info);
        }
        Self {
            counters: Counters::new(),
            last_time: Instant::now(),
            last_system: None,
            last_process: HashMap::new(),
            last_network: HashMap::new(),
            processors: info.dwNumberOfProcessors.max(1) as f64,
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f64();
        self.last_time = now;
        let mut result = Snapshot::default();
        unsafe {
            let mut idle = zeroed();
            let mut kernel = zeroed();
            let mut user = zeroed();
            if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
                let current = (ticks(idle), ticks(kernel) + ticks(user));
                if let Some(previous) = self.last_system {
                    if let (Some(idle_delta), Some(total_delta)) = (
                        current.0.checked_sub(previous.0),
                        current.1.checked_sub(previous.1),
                    ) {
                        if total_delta > 0 {
                            result.cpu = Some(
                                100.0 * total_delta.saturating_sub(idle_delta) as f64
                                    / total_delta as f64,
                            );
                        }
                    }
                }
                self.last_system = Some(current);
            }
            let mut memory: MEMORYSTATUSEX = zeroed();
            memory.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut memory) != 0 {
                let used = memory.ullTotalPhys.saturating_sub(memory.ullAvailPhys) as f64;
                result.ram_used = Some(used);
                result.ram_total = Some(memory.ullTotalPhys as f64);
                result.ram_percent =
                    (memory.ullTotalPhys > 0).then_some(100.0 * used / memory.ullTotalPhys as f64);
            }
        }
        (result.download, result.upload) = self.network(elapsed);
        result.processes = self.processes(elapsed);
        if self.counters.collect() {
            result.read = Counters::scalar(self.counters.read);
            result.write = Counters::scalar(self.counters.write);
            if let Some(values) = Counters::array(self.counters.vram) {
                result.vram_used = Some(values.iter().map(|(_, v)| v).sum());
            }
            if let Some(values) = Counters::array(self.counters.gpu) {
                // Sum processes on the SAME physical engine, then take the
                // busiest engine. Adding different engines would exceed 100%.
                let mut engines: HashMap<String, f64> = HashMap::new();
                let mut per_pid: HashMap<u32, HashMap<String, f64>> = HashMap::new();
                for (name, value) in values {
                    let Some((prefix, engine)) = name.split_once("_luid_") else {
                        continue;
                    };
                    let Some(pid) = prefix
                        .strip_prefix("pid_")
                        .and_then(|p| p.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    *engines.entry(engine.to_owned()).or_default() += value;
                    *per_pid
                        .entry(pid)
                        .or_default()
                        .entry(engine.to_owned())
                        .or_default() += value;
                }
                result.gpu = engines
                    .values()
                    .copied()
                    .reduce(f64::max)
                    .map(|v| v.clamp(0.0, 100.0));
                for process in &mut result.processes {
                    // An absent instance is unknown, not a fabricated zero.
                    process.gpu = per_pid
                        .get(&process.pid)
                        .and_then(|e| e.values().copied().reduce(f64::max))
                        .map(|v| v.clamp(0.0, 100.0));
                }
            }
        }
        result
    }

    fn network(&mut self, seconds: f64) -> (Option<f64>, Option<f64>) {
        unsafe {
            let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
            if GetIfTable2(&mut table) != 0 || table.is_null() {
                self.last_network.clear();
                return (None, None);
            }
            let rows = std::slice::from_raw_parts(
                std::ptr::addr_of!((*table).Table).cast(),
                (*table).NumEntries as usize,
            );
            let mut current = HashMap::new();
            let (mut down, mut up, mut valid) = (0.0, 0.0, false);
            for row in rows {
                let row: &windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2 = row;
                // Connected hardware interfaces only; excludes VPN/tunnel and loopback duplication.
                if row.OperStatus != 1
                    || row.Type == 24
                    || row.Type == 131
                    || row.InterfaceAndOperStatusFlags._bitfield & 1 == 0
                {
                    continue;
                }
                let id = row.InterfaceLuid.Value;
                current.insert(id, (row.InOctets, row.OutOctets));
                if let Some(previous) = self.last_network.get(&id) {
                    if let (Some(d), Some(u)) = (
                        delta_rate(previous.0, row.InOctets, seconds),
                        delta_rate(previous.1, row.OutOctets, seconds),
                    ) {
                        down += d;
                        up += u;
                        valid = true;
                    }
                }
            }
            FreeMibTable(table.cast());
            self.last_network = current;
            (valid.then_some(down), valid.then_some(up))
        }
    }

    fn processes(&mut self, seconds: f64) -> Vec<Process> {
        let mut result = Vec::new();
        let mut current = HashMap::new();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                self.last_process.clear();
                return result;
            }
            let mut entry: PROCESSENTRY32W = zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snapshot, &mut entry);
            while ok != 0 {
                if entry.th32ProcessID != 0 {
                    let length = entry
                        .szExeFile
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
                    let mut process = Process {
                        pid: entry.th32ProcessID,
                        name: name.clone(),
                        key: name.to_lowercase(),
                        ..Default::default()
                    };
                    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process.pid);
                    if !handle.is_null() {
                        let mut path = vec![0u16; 32768];
                        let mut length = path.len() as u32;
                        if QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length)
                            != 0
                        {
                            process.key =
                                String::from_utf16_lossy(&path[..length as usize]).to_lowercase();
                        }
                        let (mut created, mut exit, mut kernel, mut user) =
                            (zeroed(), zeroed(), zeroed(), zeroed());
                        if GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user)
                            != 0
                        {
                            process.identity = ticks(created);
                            let key = (process.pid, process.identity);
                            let total = ticks(kernel) + ticks(user);
                            if let Some(previous) = self.last_process.get(&key) {
                                process.cpu = delta_rate(*previous, total, seconds).map(|rate| {
                                    (rate / 100_000.0 / self.processors).clamp(0.0, 100.0)
                                });
                            }
                            current.insert(key, total);
                        }
                        CloseHandle(handle);
                    }
                    let handle =
                        OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process.pid);
                    if !handle.is_null() {
                        let mut memory: PROCESS_MEMORY_COUNTERS = zeroed();
                        if GetProcessMemoryInfo(
                            handle,
                            &mut memory,
                            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                        ) != 0
                        {
                            process.ram = Some(memory.WorkingSetSize as f64);
                        }
                        CloseHandle(handle);
                    }
                    result.push(process);
                }
                ok = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        self.last_process = current;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_handle_reset_and_actual_elapsed_time() {
        assert_eq!(delta_rate(100, 500, 2.0), Some(200.0));
        assert_eq!(delta_rate(500, 100, 1.0), None);
        assert_eq!(delta_rate(100, 500, 0.0), None);
    }
}
