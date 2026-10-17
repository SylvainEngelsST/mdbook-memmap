use std::{env, fs, io, path::Path};

fn sources(root: &Path, paths: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            sources(&path, paths)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            paths.push(
                path.to_str()
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 source filename")
                    })?
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut paths = vec!["Cargo.toml".into(), "Cargo.lock".into(), "build.rs".into()];
    sources(Path::new("src"), &mut paths)?;
    paths.sort();
    let mut output = String::from("const IMPLEMENTATION_FILES: &[(&str, &[u8])] = &[\n");
    for path in paths {
        println!("cargo:rerun-if-changed={path}");
        output.push_str(&format!(
            "({path:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/\", {path:?}))),\n"
        ));
    }
    output.push_str("];\n");
    println!("cargo:rerun-if-changed=src");
    fs::write(
        Path::new(
            &env::var_os("OUT_DIR")
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is missing"))?,
        )
        .join("implementation.rs"),
        output,
    )
}
