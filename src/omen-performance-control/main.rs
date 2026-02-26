use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::thread::sleep;
use std::time::{self, Duration};

const PERFORMANCE_OFFSET: u64 = 0x95;
const TIMER_OFFSET: u64 = 0x63;
const ECIO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";
const SLEEP: Duration = Duration::from_millis(500);

fn main() {
    let mode: String = std::env::args().nth(1).expect("no mode given");
    let mut ecio = File::options()
        .read(true)
        .write(true)
        .open(ECIO_FILE)
        .expect("Failed to open file");
    ecio.seek(SeekFrom::Start(PERFORMANCE_OFFSET))
        .expect("Failed to seek offset");
    if mode == "performance" {
        ecio.write_all(&[0x31]).expect("Unable to write to file");
        println!("Set to performance mode");
    } else if mode == "powersave" {
        ecio.write_all(&[0x30]).expect("Unable to write to file");
        println!("Set to powersave mode");
    } else {
        eprintln!(
            "Invalid mode: {}. Valid modes are: performance, powersave",
            mode
        );
        return;
    }
    ecio.flush().expect("Unable to sync");
    sleep(SLEEP);
    ecio.seek(SeekFrom::Start(TIMER_OFFSET))
        .expect("Failed to seek offset");
    ecio.write_all(&[0x00]).expect("Unable to write to file");
    ecio.flush().expect("Unable to sync");
}
