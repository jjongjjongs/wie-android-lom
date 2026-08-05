use std::{env, fs, path::Path};

use smaf_player::{SmafEvent, parse_smaf};

fn fnv1a(samples: &[i16]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for sample in samples {
        for byte in sample.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn hex_prefix(data: &[u8], count: usize) -> String {
    data.iter().take(count).map(|byte| format!("{byte:02x}")).collect()
}

fn main() {
    let directory = env::args().nth(1).expect("usage: mmf_signatures <sound-directory>");
    let mut paths = fs::read_dir(directory)
        .expect("sound directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mmf"))
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        let bytes = fs::read(&path).expect("MMF file");
        let name = Path::new(&path).file_stem().unwrap().to_string_lossy();
        for (_, event) in parse_smaf(&bytes) {
            match event {
                SmafEvent::Wave { channel, sampling_rate, data } => println!(
                    "{name},wave,{channel},{sampling_rate},{},{:016x}",
                    data.len(),
                    fnv1a(&data)
                ),
                SmafEvent::MidiSysEx(data) => {
                    println!("{name},sysex,{}", hex_prefix(&data, 16));
                }
                _ => {}
            }
        }
    }
}
