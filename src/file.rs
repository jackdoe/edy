use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Loaded {
    pub content: String,
    pub mode: Option<u32>,
    pub path: PathBuf,
}

fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

pub fn canonical(path: &Path) -> PathBuf {
    if let Ok(p) = path.canonicalize() {
        return p;
    }
    let name = match path.file_name() {
        Some(n) => n,
        None => return path.to_path_buf(),
    };
    match dir_of(path).canonicalize() {
        Ok(d) => d.join(name),
        Err(_) => path.to_path_buf(),
    }
}

pub fn load(path: &Path) -> io::Result<Loaded> {
    let path = canonical(path);
    let meta = fs::metadata(&path)?;
    if meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "is a directory"));
    }
    let bytes = fs::read(&path)?;
    let content = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8"))?;
    Ok(Loaded { content, mode: Some(meta.permissions().mode()), path })
}

pub fn complete(partial: &str) -> Vec<String> {
    let (dir_part, prefix) = match partial.rfind('/') {
        Some(i) => partial.split_at(i + 1),
        None => ("", partial),
    };
    let dir = if dir_part.is_empty() { Path::new(".") } else { Path::new(dir_part) };
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with(prefix) {
                continue;
            }
            if name.starts_with('.') && !prefix.starts_with('.') {
                continue;
            }
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            out.push(format!("{}{}{}", dir_part, name, if is_dir { "/" } else { "" }));
        }
    }
    out.sort();
    out
}

pub fn save(path: &Path, content: &str, mode: Option<u32>) -> io::Result<()> {
    let dir = dir_of(path);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("edy");
    let mut tmp = PathBuf::new();
    let mut file = None;
    for i in 0u32.. {
        let cand = dir.join(format!(".{}.{}.{}.edy", name, std::process::id(), i));
        match fs::OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(f) => {
                tmp = cand;
                file = Some(f);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let mut file = file.unwrap();
    let result = (|| {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        file.set_permissions(fs::Permissions::from_mode(mode.unwrap_or(0o600) & 0o7777))?;
        drop(file);
        fs::rename(&tmp, path)?;
        fs::File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_completion() {
        let dir = std::env::temp_dir().join(format!("edy_complete_{}", std::process::id()));
        fs::create_dir_all(dir.join("subdir")).unwrap();
        fs::write(dir.join("apple.txt"), "").unwrap();
        fs::write(dir.join("apricot.txt"), "").unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();
        let base = format!("{}/", dir.display());

        let aps = complete(&format!("{}ap", base));
        assert_eq!(aps.len(), 2);
        assert!(aps[0].ends_with("apple.txt"));
        assert!(aps[1].ends_with("apricot.txt"));

        let subs = complete(&format!("{}su", base));
        assert_eq!(subs.len(), 1);
        assert!(subs[0].ends_with("subdir/"));

        let all = complete(&base);
        assert_eq!(all.len(), 3);

        let hidden = complete(&format!("{}.h", base));
        assert_eq!(hidden.len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }
}
