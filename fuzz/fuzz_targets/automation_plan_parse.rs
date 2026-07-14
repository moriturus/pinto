#![no_main]

use libfuzzer_sys::fuzz_target;
use pinto::automation::AutomationPlan;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = AutomationPlan::parse(input);
    }
});
