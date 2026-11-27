//! Process-local build snapshot: installing a deb must not remove lazy chunks
//! still referenced by pages belonging to the old, running backend.
use std::{collections::HashMap, io, path::Path};

pub struct StaticAssets(HashMap<String, Vec<u8>>);
impl StaticAssets {
    pub fn load(root: &Path) -> io::Result<Self> {
        fn visit(root: &Path, dir: &Path, files: &mut HashMap<String, Vec<u8>>, total: &mut usize) -> io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_dir() { visit(root, &entry.path(), files, total)?; }
                else if kind.is_file() {
                    let bytes = std::fs::read(entry.path())?;
                    *total += bytes.len();
                    if *total > 128 * 1024 * 1024 {
                        return Err(io::Error::other("frontend assets exceed snapshot limit"));
                    }
                    let path = entry.path();
                    let name = path.strip_prefix(root).map_err(io::Error::other)?.to_string_lossy().replace('\\', "/");
                    files.insert(name, bytes);
                }
            }
            Ok(())
        }
        let mut files = HashMap::new();
        visit(root, root, &mut files, &mut 0)?;
        if !files.contains_key("index.html") { return Err(io::Error::other("frontend index.html missing")); }
        Ok(Self(files))
    }
    pub fn get(&self, request: &str) -> Option<(&[u8], &str)> {
        let name = request.trim_start_matches('/');
        let name = if name.is_empty() { "index.html" } else { name };
        self.0.get_key_value(name).map(|(name, bytes)| (bytes.as_slice(), name.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrade_cannot_mix_a_running_pages_build_with_new_chunks() {
        let root = std::env::temp_dir().join(format!("openless-static-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), b"old index").unwrap();
        std::fs::write(root.join("assets/Style-old.js"), b"old style").unwrap();
        let snapshot = StaticAssets::load(&root).unwrap();
        std::fs::remove_file(root.join("assets/Style-old.js")).unwrap();
        std::fs::write(root.join("index.html"), b"new index").unwrap();
        std::fs::write(root.join("assets/Style-new.js"), b"new style").unwrap();
        assert_eq!(snapshot.get("/").unwrap().0, b"old index");
        assert_eq!(snapshot.get("/assets/Style-old.js").unwrap().0, b"old style");
        assert!(snapshot.get("/assets/Style-new.js").is_none());
        assert!(snapshot.get("/../../etc/passwd").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
