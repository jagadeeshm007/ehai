//! Extract the `ehai`/`ehai.exe` binary from a downloaded release archive.
//! `.zip` for Windows assets, `.tar.gz` for everything else — matching
//! `.github/workflows/release.yml`'s packaging.

use std::io::{Cursor, Read};

use crate::{Error, Result};

pub fn extract_binary(archive: &[u8], asset_name: &str) -> Result<Vec<u8>> {
    if asset_name.ends_with(".zip") {
        extract_zip(archive)
    } else {
        extract_tar_gz(archive)
    }
}

fn is_binary_entry(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    base == "ehai" || base == "ehai.exe"
}

fn extract_zip(archive: &[u8]) -> Result<Vec<u8>> {
    let mut zip =
        zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| Error::Http(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| Error::Http(e.to_string()))?;
        if is_binary_entry(entry.name()) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(Error::io(entry.name().to_string()))?;
            return Ok(buf);
        }
    }
    Err(Error::BinaryMissing)
}

fn extract_tar_gz(archive: &[u8]) -> Result<Vec<u8>> {
    let gz = flate2::read::GzDecoder::new(Cursor::new(archive));
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries().map_err(Error::io("archive"))? {
        let mut entry = entry.map_err(Error::io("archive entry"))?;
        let name = entry
            .path()
            .map_err(Error::io("archive entry path"))?
            .to_string_lossy()
            .into_owned();
        if is_binary_entry(&name) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(Error::io(name))?;
            return Ok(buf);
        }
    }
    Err(Error::BinaryMissing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (name, data) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(name).unwrap();
                header.set_size(data.len() as u64);
                header.set_cksum();
                builder.append(&header, *data).unwrap();
            }
            builder.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap()
    }

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
            for (name, data) in entries {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn finds_the_binary_in_a_tar_gz_with_a_nested_dir() {
        let archive = make_tar_gz(&[
            ("ehai-0.1.0-x86_64/README.md", b"readme"),
            ("ehai-0.1.0-x86_64/ehai", b"binary-bytes"),
        ]);
        let bin = extract_binary(&archive, "ehai-0.1.0-x86_64.tar.gz").unwrap();
        assert_eq!(bin, b"binary-bytes");
    }

    #[test]
    fn finds_the_exe_in_a_zip() {
        let archive = make_zip(&[
            ("ehai-0.1.0-win/ehai.exe", b"win-binary"),
            ("ehai-0.1.0-win/README.md", b"readme"),
        ]);
        let bin = extract_binary(&archive, "ehai-0.1.0-win.zip").unwrap();
        assert_eq!(bin, b"win-binary");
    }

    #[test]
    fn missing_binary_is_an_error() {
        let archive = make_tar_gz(&[("stuff/README.md", b"readme")]);
        assert!(matches!(
            extract_binary(&archive, "stuff.tar.gz"),
            Err(Error::BinaryMissing)
        ));
    }
}
