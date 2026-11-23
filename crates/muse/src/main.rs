use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "用法: muse check [诗词包目录，默认 packs]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => check(args.get(1).map_or_else(|| PathBuf::from("packs"), PathBuf::from)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// 校验目录下所有诗词包，供编写自定义包时使用。
fn check(root: PathBuf) -> ExitCode {
    let (packs, errors) = match muse_core::load_all(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    for p in &packs {
        let poem = &p.pack.poem;
        println!("ok    {}（{}·{}）", poem.title, poem.dynasty, poem.author);
    }
    for e in &errors {
        eprintln!("error {e}");
    }
    println!("{} 个通过，{} 个失败", packs.len(), errors.len());
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
