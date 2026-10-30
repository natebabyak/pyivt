//! Narrow entry points for cargo-fuzz; absent from normal builds and Python API.

pub fn decompress_component(data: &[u8]) {
    let _ = crate::decode::decompress_component(data);
}

pub fn metadata_directory(data: &[u8]) {
    use crate::metadata_directory::MetadataDirectory;
    for slot in [544, 558, 824, 838, 852] {
        if let Ok(directory) = MetadataDirectory::new(data, slot) {
            for index in [0, 1, directory.count.saturating_sub(1)] {
                let _ = directory.entry(data, index);
            }
            let source = crate::io::IvtSource::from_bytes(data.to_vec());
            let _ = directory.read(&source);
        }
    }
}

pub fn decode_page(data: &[u8]) {
    crate::decode::fuzz_page(data);
}

pub fn read_members(data: &[u8]) {
    for (axis, count) in [(0, 2), (1, 2), (2, 3)] {
        let _ = crate::members::read_members(data, axis, count);
    }
}
