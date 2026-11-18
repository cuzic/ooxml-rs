//! Replays fuzz inputs on stable Rust and prints one line per input: the
//! panic location and message, or `ok`. Used to triage and bucket
//! `fuzz/artifacts/*` (stack overflows and aborts still kill the process;
//! run those one at a time).
//!
//! ```text
//! cargo run -p ooxml-robustness --example replay -- <target> <file>…
//! ```
//!
//! Targets: package_roundtrip, xml_t1, xml_t1b, lens_ops, xlsx_edit, pptx_edit.

use std::panic;
use std::sync::{Arc, Mutex};

fn run(target: &str, data: &[u8]) {
    match target {
        "package_roundtrip" => ooxml_robustness::check_package(data, (data.len() % 251) as u8),
        "xml_t1" => ooxml_robustness::check_t1(data),
        "xml_t1b" => ooxml_robustness::check_t1b(data),
        "lens_ops" => ooxml_robustness::check_lens_ops(data),
        "xlsx_edit" => ooxml_robustness::check_xlsx_input(data),
        "pptx_edit" => ooxml_robustness::check_pptx_input(data),
        other => panic!("unknown target {other}"),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().expect("target");
    let last: Arc<Mutex<String>> = Arc::default();
    let l2 = last.clone();
    panic::set_hook(Box::new(move |info| {
        let msg = info.payload_as_str().unwrap_or("<non-string payload>");
        let first = if std::env::var_os("REPLAY_FULL").is_some() { msg } else { msg.lines().next().unwrap_or("") };
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        *l2.lock().unwrap() = format!("{loc} {first}");
    }));
    for f in args {
        let data = std::fs::read(&f).expect("read input");
        let r = panic::catch_unwind(|| run(&target, &data));
        match r {
            Ok(()) => println!("ok\t{f}"),
            Err(_) => println!("PANIC\t{f}\t{}", last.lock().unwrap()),
        }
    }
}
