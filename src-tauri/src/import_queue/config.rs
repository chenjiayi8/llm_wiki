use std::{env, fs, path::Path};

pub const DEFAULT_IMPORT_MAX_CONCURRENCY: usize = 5;

pub fn load_import_max_concurrency() -> usize {
    load_from_dotenv_file(Path::new("../.env"))
        .or_else(|| env::var("IMPORT_MAX_CONCURRENCY").ok())
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_IMPORT_MAX_CONCURRENCY)
}

fn load_from_dotenv_file(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .find_map(|line| line.strip_prefix("IMPORT_MAX_CONCURRENCY="))
        .map(|value| value.trim().trim_matches('"').to_string())
}
