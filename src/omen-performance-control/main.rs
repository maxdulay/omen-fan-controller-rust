use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

const PERFORMANCE_OFFSET: u64 = 0x95;
const ECIO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";

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
        eprintln!("Invalid mode: {}. Valid modes are: performance, powersave", mode);
        return;
    }
    ecio.flush().expect("Unable to sync");
}
