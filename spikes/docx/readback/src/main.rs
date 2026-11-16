//! Evaluate candidate independent readers for ADR-0004's read-back oracle.
//! Usage: cargo run --release -- <out dir> <file.docx>...
//! Writes <out>/<name>.docx-rs.json (docx-rs `read_docx(..).json()`) and
//! <out>/<name>.docx-rust.txt (docx-rust `Body::text()`), and prints one status line per file.
use std::{fs, panic, path::Path};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = Path::new(&args[0]);
    fs::create_dir_all(out).unwrap();
    panic::set_hook(Box::new(|_| {}));
    for f in &args[1..] {
        let name = Path::new(f).file_name().unwrap().to_string_lossy().into_owned();
        let buf = fs::read(f).unwrap();
        let a = panic::catch_unwind(|| docx_rs::read_docx(&buf).map(|d| d.json()));
        let sa = match a {
            Ok(Ok(j)) => { fs::write(out.join(format!("{name}.docx-rs.json")), j).unwrap(); "ok".to_string() }
            Ok(Err(e)) => format!("err({e})"),
            Err(_) => "panic".to_string(),
        };
        let p = f.clone();
        let b = panic::catch_unwind(move || {
            let file = docx_rust::DocxFile::from_file(&p).map_err(|e| format!("{e:?}"))?;
            let doc = file.parse().map_err(|e| format!("{e:?}"))?;
            Ok::<String, String>(doc.document.body.text())
        });
        let sb = match b {
            Ok(Ok(t)) => { fs::write(out.join(format!("{name}.docx-rust.txt")), t).unwrap(); "ok".to_string() }
            Ok(Err(e)) => format!("err({})", &e[..e.len().min(80)]),
            Err(_) => "panic".to_string(),
        };
        println!("{name}\tdocx-rs={sa}\tdocx-rust={sb}");
    }
}
