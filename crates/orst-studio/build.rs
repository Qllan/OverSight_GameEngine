//! ORST.exe exports the engine's C API, so C# scripts hosted inside the
//! editor call the very engine the editor shows (their DllImport of
//! "orst" resolves to the editor itself). The names come from orst.h.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let header = PathBuf::from("../orst/include/orst.h");
    println!("cargo:rerun-if-changed={}", header.display());
    let text = fs::read_to_string(&header).expect("orst.h");
    let names = functions(&text);
    assert!(!names.is_empty(), "no functions found in orst.h");

    // The list again for the editor, which writes it into an import
    // library so C++ scripts can link against ORST.exe on Windows
    let list: String = names.iter().map(|n| format!("    {n:?},\n")).collect();
    let rs = PathBuf::from(env::var("OUT_DIR").unwrap()).join("exports.rs");
    fs::write(rs, format!("pub const EXPORTS: &[&str] = &[\n{list}];\n")).unwrap();

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc";
    if os == "windows" {
        // A module definition file works with both link.exe and GNU ld
        let def = PathBuf::from(env::var("OUT_DIR").unwrap()).join("orst_exports.def");
        let mut body = String::from("EXPORTS\n");
        for n in &names {
            body += &format!("    {n}\n");
        }
        fs::write(&def, body).unwrap();
        if msvc {
            println!("cargo:rustc-link-arg=/DEF:{}", def.display());
        } else {
            println!("cargo:rustc-link-arg={}", def.display());
        }
    } else {
        for n in &names {
            // Keep the function though nothing in the editor calls it,
            // and put it in the dynamic symbol table
            println!("cargo:rustc-link-arg=-Wl,--undefined={n}");
            if os == "macos" {
                println!("cargo:rustc-link-arg=-Wl,-exported_symbol,_{n}");
            } else {
                println!("cargo:rustc-link-arg=-Wl,--export-dynamic-symbol={n}");
            }
        }
    }
}

/// Every `orst_name(` declared in the header
fn functions(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        // Comments, macros and header-only helpers are not in the library
        if line.starts_with("/*")
            || line.starts_with('*')
            || line.starts_with('#')
            || line.starts_with("static inline")
        {
            continue;
        }
        let mut rest = line;
        while let Some(i) = rest.find("orst_") {
            let tail = &rest[i..];
            let end = tail
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(tail.len());
            let (name, after) = tail.split_at(end);
            let prefixed = i == 0 || !rest.as_bytes()[i - 1].is_ascii_alphanumeric();
            if prefixed && after.starts_with('(') && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
            rest = after;
        }
    }
    names
}
