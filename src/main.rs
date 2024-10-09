use signal_hook::consts::{SIGTERM, SIGINT};
use signal_hook::iterator::Signals;
use std::thread;
use bisection::bisect_left;
use lzma_rs;
use nix::{kmod::*, unistd::Uid, sys::utsname::uname};
use std::ffi::CString;
use std::fs::{read_to_string, File};
use std::io::prelude::*;
use std::io::{Seek, SeekFrom};
use std::thread::sleep;
use std::panic;
use toml;

const FAN1_OFFSET: u64 = 52;
const FAN2_OFFSET: u64 = 53;
const BIOS_OFFSET: u8 = 98;
const TIMER_OFFSET: u8 = 99;
const GPU_TEMP_OFFSET: u64 = 183;
const ECIO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";
const CONFIG_FILE: &str = "/etc/omen-fan/config.toml";
const CPU_TEMP_OFFSET: u64 = 87;
const FAN1_MAX: u8 = 55;
const FAN2_MAX: u8 = 57;

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
    let fmod = File::open(
        format!("/run/booted-system/kernel-modules/lib/modules/{}/kernel/drivers/acpi/ec_sys.ko.xz", sysinfo.release().to_str().expect("Could not get version"))
    )
    .expect("Did not find kernel module");
    let mut f = std::io::BufReader::new(fmod);
    let mut decomp: Vec<u8> = Vec::new();
    lzma_rs::xz_decompress(&mut f, &mut decomp).unwrap();
    let param = &CString::new("write_support=1").unwrap();
    init_module(&decomp, &*param).expect("Load module");
}

fn get_temp() -> u8 {
    let mut ecio = File::options()
        .read(true)
        .write(true)
        .open(ECIO_FILE)
        .expect("Failed to open file");
    _ = ecio.seek(SeekFrom::Start(CPU_TEMP_OFFSET));
    let mut cputtemp = [0];
    ecio.read_exact(&mut cputtemp).expect("Unable to read temp");
    _ = ecio.seek(SeekFrom::Start(GPU_TEMP_OFFSET));
    let mut gputemp = [0];
    ecio.read_exact(&mut gputemp).expect("Unable to read temp");
    cputtemp[0].max(gputemp[0])
}

fn update_fan(speed1: u8, speed2: u8) {
    let mut ecio = File::options()
        .read(true)
        .write(true)
        .open(ECIO_FILE)
        .expect("Failed to open file");
    ecio.seek(SeekFrom::Start(FAN1_OFFSET))
        .expect("Could not seek fan1 offset");
    ecio.write_all(&[speed1]).expect("Unable to write to file");
    ecio.seek(SeekFrom::Start(FAN2_OFFSET))
        .expect("Could not seek fan2 offset");
    ecio.write_all(&[speed2]).expect("Unable to write to file");
    ecio.flush().expect("Unable to sync");
}

fn bios_control( enabled: bool) {
    let mut ecio = File::options()
        .read(true)
        .write(true)
        .open(ECIO_FILE)
        .expect("Failed to open file");
    ecio.seek(SeekFrom::Start(BIOS_OFFSET as u64))
        .expect("Unable to seek");
    if enabled == true {
        ecio.write_all(&[0]).expect("Unable to write to file");
        println!("BIOS control is enabled");
    } else {
        ecio.write_all(&[6]).expect("Unable to write to file");
        ecio.seek(SeekFrom::Start(TIMER_OFFSET as u64))
            .expect("Unable to seek");
        ecio.write_all(&[0]).expect("Unable to write to file");
        println!("WARNING: BIOS control is disabled");
    }
    ecio.flush().expect("Unable to sync");
}

fn check_root() {
    if !Uid::effective().is_root() {
        panic!("Must be run as root");
    }
}

fn main() {
// Create a Signals instance to handle signals
    {
        check_root();
        load_ec_sys();
    }
    let config = File::open(CONFIG_FILE);
    let mut contents = String::new();
    let _ = config
        .expect("Config file not found")
        .read_to_string(&mut contents);
    let configtable = contents.parse::<toml::Table>().unwrap();
    let temp_curve: Vec<Vec<u8>> = configtable["service"]["TEMP_CURVE"]
        .as_array()
        .expect("Couldn't convert temp_curve to array")
        .into_iter()
        .map(|inner_array| {
            inner_array
                .as_array()
                .unwrap()
                .into_iter()
                .map(|x| x.as_integer().expect("Could not convert") as u8)
                .collect()
        })
        .collect();
    let speed_curve: Vec<Vec<u8>> = configtable["service"]["SPEED_CURVE"]
        .as_array()
        .expect("Couldn't convert speed_curve to array")
        .into_iter()
        .map(|percentspeed| percentspeed.as_integer().expect("Convert to float") as f64)
        .map(|percentspeed| {
            vec![
                ({ percentspeed * 0.01 * FAN1_MAX as f64 }) as u8,
                ({ percentspeed * 0.01 * FAN2_MAX as f64 }) as u8,
            ]
        })
        .collect();
    let temp_curve_high: Vec<u8> = temp_curve.clone().into_iter().map(|x| x[1]).collect();
    let poll_interval = std::time::Duration::from_millis(
        configtable["service"]["POLL_INTERVAL"]
            .as_integer()
            .expect("POLL_INTERVAL not found in config file") as u64,
    );

    drop(configtable);

    let mut lowthreshold: u8 = 100;
    let mut highthreshold: u8 = 0;
    let mut index: usize = 1;
    let mut temp: u8 = get_temp();
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
                },
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
        temp = get_temp();
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
            update_fan(speed_curve[index][0], speed_curve[index][1]);
        }
        sleep(poll_interval);
    }

}

