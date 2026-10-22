//! Arbitrary text into the parser: it must never panic, and anything it
//! accepts must survive print -> parse unchanged.
#![no_main]

use astfuzz::lang::parser::parse;
use astfuzz::lang::printer::print_program;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(p) = parse(text) {
        let printed = print_program(&p);
        match parse(&printed) {
            Ok(back) => assert_eq!(back, p, "round trip changed the program:\n{printed}"),
            Err(e) => panic!("printed program does not parse: {e}\n{printed}"),
        }
    }
});
