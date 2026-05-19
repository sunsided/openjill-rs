use assert2::check;
use openjill_data::vcl::VclFile;
use openjill_data::DataDirectory;
use std::path::{Path, PathBuf};

const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

#[test]
fn parses_original_jill_vcl_text_entries_when_available() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is not set and default data directory is missing"
            );
            return;
        }
    };

    check!(
        data_dir.is_dir(),
        "data directory must exist when configured: {}",
        data_dir.display()
    );

    let directory = DataDirectory::new(&data_dir);
    let mut reader = directory.open_reader("JILL1.VCL").unwrap_or_else(|error| {
        panic!(
            "JILL1.VCL must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let file_len = reader.len();

    let vcl = VclFile::parse(&mut reader).expect("JILL1.VCL from original data should parse");
    check!(
        !vcl.text_entries().is_empty(),
        "JILL1.VCL should contain non-empty text entries"
    );
    check!(vcl.text_entry_count() == vcl.text_entries().len());

    for entry in vcl.text_entries() {
        check!(
            entry.offset() < file_len,
            "entry offset must point into file: {}",
            entry.offset()
        );
        check!(
            !entry.text().is_empty(),
            "parsed text entries should be non-empty"
        );
    }
}

fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path));
    }

    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    if default.is_dir() {
        Some(default)
    } else {
        None
    }
}
