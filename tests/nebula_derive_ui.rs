#![cfg(feature = "nebula")]

#[test]
fn nebula_derive_compile_failures() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/nebula_derive_*.rs");
}
