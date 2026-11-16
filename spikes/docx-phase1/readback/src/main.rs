//! What docx-rs 0.4.22 reads outside `document.children` (ADR-0007 §9, T5 analogue).
//! Usage: cargo run --release -- <out dir> <file.docx>...
//! Writes <out>/<name>.json (`read_docx(..).json()`) and prints one status line per file;
//! `readback_stories.py` compares the JSON with an ElementTree reading of each story part.
use std::{fs, panic, path::Path};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = Path::new(&args[0]);
    fs::create_dir_all(out).unwrap();
    panic::set_hook(Box::new(|_| {}));
    for f in &args[1..] {
        let name = Path::new(f).file_name().unwrap().to_string_lossy().into_owned();
        let buf = fs::read(f).unwrap();
        let r = panic::catch_unwind(|| docx_rs::read_docx(&buf).map(|d| d.json()));
        let s = match r {
            Ok(Ok(j)) => {
                fs::write(out.join(format!("{name}.json")), j).unwrap();
                "ok".to_string()
            }
            Ok(Err(e)) => format!("err({e})"),
            Err(_) => "panic".to_string(),
        };
        println!("{name}\t{s}");
    }
}
