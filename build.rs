use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/app-icon.ico")
            .set("ProductName", "Frame Record")
            .set("FileDescription", "Frame Record")
            .set("OriginalFilename", "frame-record.exe")
            .compile()?;
    }
    let mut files: Vec<PathBuf> = [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "README.md",
        "LICENSE",
        "THIRD-PARTY.md",
        ".gitignore",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    for directory in ["src", "assets", "scripts"] {
        collect_source_files(Path::new(directory), &mut files)?;
    }
    files.sort();
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let mut archive =
        zip::ZipWriter::new(fs::File::create(output.join("frame-record-source.zip"))?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9))
        .unix_permissions(0o644);
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let name = format!("Frame Record/{}", file.to_string_lossy().replace('\\', "/"));
        archive.start_file(name, options)?;
        archive.write_all(&fs::read(file)?)?;
    }
    archive.finish()?;
    Ok(())
}

fn collect_source_files(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={}", directory.display());
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            collect_source_files(&path, files)?;
        } else if kind.is_file()
            && matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "ps1" | "py" | "svg" | "png" | "ico")
            )
        {
            files.push(path);
        }
    }
    Ok(())
}
