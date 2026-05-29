use super::*;

// Go: internal/ast/functionflags.go:FunctionFlags
#[test]
fn function_flags_base_and_union() {
    assert_eq!(FunctionFlags::NORMAL.bits(), 0);
    assert_eq!(FunctionFlags::GENERATOR.bits(), 1 << 0);
    assert_eq!(FunctionFlags::ASYNC.bits(), 1 << 1);
    assert_eq!(FunctionFlags::INVALID.bits(), 1 << 2);
    assert_eq!(
        FunctionFlags::ASYNC_GENERATOR,
        FunctionFlags::ASYNC | FunctionFlags::GENERATOR
    );
}
