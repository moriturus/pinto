#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use pinto::storage::parse_item_markdown;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = parse_item_markdown(input, Path::new("fuzz-input.md"));
    }
});
