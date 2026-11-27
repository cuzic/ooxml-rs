use std::path::{Path, PathBuf};

pub(crate) fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
            matches!(
                x,
                "xlsx" | "xlsm" | "pptx" | "pptm" | "ppsx" | "potx" | "docx" | "docm"
            )
        }) {
            out.push(p);
        }
    }
}
