use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use std::{
    process::{Command, Stdio},
    sync::Mutex,
    time::SystemTime,
};
use sysinfo::{CpuExt, System, SystemExt};

lazy_static! {
    static ref SYS: Mutex<System> = Mutex::new(System::new_all());
}

const SUFFIX: [&str; 9] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];

pub trait WBarModuleTrait {
    fn update(&self) -> String;
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct WBarModMask(u32);
impl WBarModMask {
    pub const VOL: Self = Self(1 << 0);
    pub const RAM: Self = Self(1 << 1);
    pub const CPU: Self = Self(1 << 2);
    pub const DATE: Self = Self(1 << 3);
    pub const TIME: Self = Self(1 << 4);
}

impl std::ops::BitOr for WBarModMask {
    type Output = WBarModMask;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for WBarModMask {
    type Output = bool;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0) == rhs
    }
}

pub struct WBarModule(pub Box<dyn WBarModuleTrait>);
impl WBarModule {
    pub fn vol() -> Self {
        Self(Box::new(WBarVol))
    }

    pub fn ram() -> Self {
        Self(Box::new(WBarRAM))
    }

    pub fn cpu() -> Self {
        Self(Box::new(WBarCPU))
    }

    pub fn date() -> Self {
        Self(Box::new(WBarDate("%a, %h %d")))
    }

    pub fn time() -> Self {
        Self(Box::new(WBarTime("%I:%M %p")))
    }
}

// TODO: more modules

pub struct WBarVol;

impl WBarModuleTrait for WBarVol {
    fn update(&self) -> String {
        let amixer = Command::new("amixer")
            .args(["sget", "Master"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let awk = Command::new("awk")
            .args(["-F", "[][]", "/Left:/ { print $2 }"])
            .stdin(Stdio::from(amixer.stdout.unwrap()))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let result = if let Ok(output) = awk.wait_with_output() {
            if output.stdout.is_empty() {
                " 0%".into()
            } else {
                format!("{: >3}", String::from_utf8(output.stdout).unwrap())
            }
        } else {
            "N/A".into()
        };
        format!("vol: {result}")
    }
}

pub struct WBarRAM;

impl WBarModuleTrait for WBarRAM {
    fn update(&self) -> String {
        let mut sys = SYS.lock().unwrap();
        sys.refresh_memory();
        let total = sys.total_memory();
        let used = sys.used_memory();

        let u = used as f64;
        let used_human = if u == 0.0 {
            "0 B".to_string()
        } else {
            const UNIT: f64 = 1024f64;
            let base = u.log10() / UNIT.log10();
            format!(
                "{} {}",
                ((UNIT.powf(base - base.floor()) * 10.0).round() / 10.0).to_string(),
                SUFFIX[base.floor() as usize]
            )
        };

        format!(
            "ram: {} ({: >2}%)",
            used_human,
            ((used as f32 / total as f32) * 100f32) as u8
        )
    }
}

pub struct WBarCPU;

impl WBarModuleTrait for WBarCPU {
    fn update(&self) -> String {
        let mut sys = SYS.lock().unwrap();
        sys.refresh_cpu();
        let used = sys.global_cpu_info().cpu_usage() as u8;
        format!("cpu: {used: >2}%")
    }
}

pub struct WBarDate(&'static str);

impl WBarModuleTrait for WBarDate {
    fn update(&self) -> String {
        let now: DateTime<Utc> = SystemTime::now().into();
        now.date_naive().format(self.0).to_string()
    }
}

pub struct WBarTime(&'static str);

impl WBarModuleTrait for WBarTime {
    fn update(&self) -> String {
        let now: DateTime<Utc> = SystemTime::now().into();
        now.format(self.0).to_string()
    }
}
