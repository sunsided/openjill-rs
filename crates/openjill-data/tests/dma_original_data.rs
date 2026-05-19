use openjill_data::dma::DmaFile;
use openjill_data::DataDirectory;
use std::path::{Path, PathBuf};

const TILESET_MASK: u8 = 0x3f;

#[test]
fn parses_original_jill_dma_when_available() {
    let data_dir = original_data_dir();
    if !data_dir.is_dir() {
        eprintln!(
            "skipping integration test; original data directory is missing: {}",
            data_dir.display()
        );
        return;
    }

    let directory = DataDirectory::new(data_dir);
    let mut reader = match directory.open_reader("JILL.DMA") {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("skipping integration test; JILL.DMA is unavailable: {error}");
            return;
        }
    };
    let file_len = reader.len();

    let dma = DmaFile::parse(&mut reader).expect("JILL.DMA from original data should parse");
    assert!(!dma.entries().is_empty(), "JILL.DMA should contain entries");
    assert_eq!(dma.entry_count(), dma.entries().len());

    for (index, entry) in dma.entries().iter().enumerate() {
        assert_eq!(entry.index(), index, "entry index should be preserved");
        assert!(entry.offset() < file_len, "entry offset must point into file");
        assert_eq!(
            entry.tileset() & !TILESET_MASK,
            0,
            "tileset must be masked to 6 bits"
        );

        assert!(
            dma.get_by_map_code(entry.map_code()).is_some(),
            "map code lookup should resolve for parsed entry"
        );
        assert!(
            dma.get_by_name(entry.name()).is_some(),
            "name lookup should resolve for parsed entry"
        );
    }

    for window in dma.entries().windows(2) {
        assert!(
            window[0].offset() < window[1].offset(),
            "entry offsets should increase monotonically"
        );
    }
}

fn original_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("OPENJILL_ORIGINAL_DATA_DIR") {
        return PathBuf::from(path);
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/original/JILL1")
        .to_path_buf()
}
