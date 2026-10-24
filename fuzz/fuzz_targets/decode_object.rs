#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_version::object::Object;

fuzz_target!(|data: &[u8]| {
    // Bytes that decode are canonical: re-encoding gives the same bytes back.
    if let Ok(object) = Object::decode(data) {
        assert_eq!(object.encode(), data);
    }
});
