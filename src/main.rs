use bisection::bisect_left;
use lzma_rs;
use nix::{kmod::*, sys::utsname::uname, unistd::Uid};
use serde::Deserialize;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::ffi::CString;
use std::fs::{read_to_string, File};
use std::io::prelude::*;
use std::io::{Seek, SeekFrom};
use std::panic;
use std::thread;
use std::thread::sleep;
use toml;

const FAN1_OFFSET: u64 = 52;
const FAN2_OFFSET: u64 = 53;
const BIOS_OFFSET: u64 = 98;
const TIMER_OFFSET: u64 = 99;
const GPU_TEMP_OFFSET: u64 = 183;
const ECIO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";
const CONFIG_FILE: &str = "/etc/omen-fan/config.toml";
const CPU_TEMP_OFFSET: u64 = 87;
const FAN1_MAX: f64 = 55.0;
const FAN2_MAX: f64 = 57.0;

struct Controller {
    ecio: File,
}

impl Controller {
    fn get_temp(&mut self) -> u8 {
        _ = self.ecio.seek(SeekFrom::Start(CPU_TEMP_OFFSET));
        let mut cputtemp = [0];
        self.ecio
            .read_exact(&mut cputtemp)
            .expect("Unable to read temp");
        _ = self.ecio.seek(SeekFrom::Start(GPU_TEMP_OFFSET));
        let mut gputemp = [0];
        self.ecio
            .read_exact(&mut gputemp)
            .expect("Unable to read temp");
        cputtemp[0].max(gputemp[0])
    }

    fn update_fan(&mut self, speed1: u8, speed2: u8) {
        self.ecio
            .seek(SeekFrom::Start(FAN1_OFFSET))
            .expect("Could not seek fan1 offset");
        self.ecio
            .write_all(&[speed1])
            .expect("Unable to write to file");
        self.ecio
            .seek(SeekFrom::Start(FAN2_OFFSET))
            .expect("Could not seek fan2 offset");
        self.ecio
            .write_all(&[speed2])
            .expect("Unable to write to file");
        self.ecio.flush().expect("Unable to sync");
    }
}

fn bios_control(enabled: bool) {
    let mut ecio = File::options()
        .read(true)
        .write(true)
        .open(ECIO_FILE)
        .expect("Failed to open file");
    ecio.seek(SeekFrom::Start(BIOS_OFFSET))
        .expect("Unable to seek");
    if enabled == true {
        ecio.write_all(&[0]).expect("Unable to write to file");
        println!("BIOS control is enabled");
    } else {
        ecio.write_all(&[6]).expect("Unable to write to file");
        ecio.seek(SeekFrom::Start(TIMER_OFFSET))
            .expect("Unable to seek");
        ecio.write_all(&[0]).expect("Unable to write to file");
        println!("WARNING: BIOS control is disabled");
    }
    ecio.flush().expect("Unable to sync");
}

fn load_ec_sys() {
    {
        let modlist = read_to_string("/proc/modules").expect("Failed to get modules list");
        if modlist.contains("ec_sys") {
            delete_module(
                &CString::new("ec_sys").unwrap(),
                DeleteModuleFlags::O_NONBLOCK,
            )
            .unwrap();
        }
    }
    let sysinfo = uname().expect("get sysinfo");
    let fmod = File::open(format!(
        "/run/booted-system/kernel-modules/lib/modules/{}/kernel/drivers/acpi/ec_sys.ko.xz",
        sysinfo.release().to_str().expect("Could not get version")
    ))
    .expect("Did not find kernel module");
    let mut f = std::io::BufReader::new(fmod);
    let mut decomp: Vec<u8> = Vec::new();
    lzma_rs::xz_decompress(&mut f, &mut decomp).unwrap();
    let param = &CString::new("write_support=1").unwrap();
    init_module(&decomp, &*param).expect("Load module");
}

fn check_root() {
    if !Uid::effective().is_root() {
        panic!("Must be run as root");
    }
}

#[derive(Deserialize)]
struct Service {
    temp_curve: [[u8; 2]; 6], // Fixed-size 2D array (6x2)
    speed_curve: [f64; 6],     // Fixed-size array of 6 elements
    poll_interval: u64,       // A single integer
}

#[derive(Deserialize)]
struct Config {
    service: Service,
}

fn main() {
    // Create a Signals instance to handle signals
    {
        check_root();
        load_ec_sys();
    }
    let toml_str = read_to_string(CONFIG_FILE).expect("Failed to read the TOML file");
    let config: Config = toml::de::from_str(&toml_str).expect("Failed to parse TOML");
    drop(toml_str);
    let temp_curve_high: [u8; 6] = [ 
        config.service.temp_curve[0][0],
        config.service.temp_curve[1][0],
        config.service.temp_curve[2][0],
        config.service.temp_curve[3][0],
        config.service.temp_curve[4][0],
        config.service.temp_curve[5][0],
    ];
    let poll_interval = std::time::Duration::from_millis(config.service.poll_interval);
    let temp_curve = config.service.temp_curve;
    let mut speed_curve: [[u8; 2]; 6] = [[0; 2]; 6];
    for i in 0..6 {
        speed_curve[i][0] = (config.service.speed_curve[i] * 0.01 * FAN1_MAX) as u8;
        speed_curve[i][1] = (config.service.speed_curve[i] * 0.01 * FAN2_MAX) as u8;
    }
    drop(config);

    let mut ecio = File::options()
        .read(true)
        .write(true)
        .open(ECIO_FILE)
        .expect("Failed to open file");
    let mut controller = Controller { ecio };
    let mut lowthreshold: u8 = 100;
    let mut highthreshold: u8 = 0;
    let mut index: usize = 1;
    let mut temp: u8 = controller.get_temp();
    let mut temp2: u8 = temp;
    let mut temp3: u8 = temp;
    let mut temp4: u8;
    let mut lastindex: usize;
    let mut mintemp: u8;
    let mut maxtemp: u8;
    let mut signals = Signals::new(&[SIGTERM, SIGINT]).expect("Failed to create signal handler");

    bios_control(false);
    let _handle = thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                SIGINT | SIGTERM => {
                    bios_control(true);
                    std::process::exit(0);
                }
                _ => unreachable!(),
            }
        }
    });
    panic::set_hook(Box::new(|_| {
        bios_control(true);
    }));
    loop {
        lastindex = index;
        temp4 = temp3;
        temp3 = temp2;
        temp2 = temp;
        temp = controller.get_temp();
        mintemp = temp.min(temp2).min(temp3).min(temp4);
        maxtemp = temp.max(temp2).max(temp3).max(temp4);
        if maxtemp <= temp_curve[0][0] {
            index = 0;
        } else if mintemp >= temp_curve[temp_curve.len() - 1][1] {
            index = speed_curve.len() - 1;
        } else if maxtemp < lowthreshold || mintemp > highthreshold {
            index = bisect_left(&temp_curve_high, &mintemp);
        }
        if index != lastindex {
            lowthreshold = temp_curve[index][0];
            highthreshold = temp_curve[index][1];
            println!("Temperature:{} Index: {}", mintemp, index);
            controller.update_fan(speed_curve[index][0], speed_curve[index][1]);
        }
        sleep(poll_interval);
    }
}
