use super::*;

// Go: internal/printer/emitflags.go (bit values match `1 << iota`)
#[test]
fn single_flag_bit_values_match_go() {
    assert_eq!(EmitFlags::SINGLE_LINE.bits(), 1 << 0);
    assert_eq!(EmitFlags::MULTI_LINE.bits(), 1 << 1);
    assert_eq!(EmitFlags::NO_LEADING_SOURCE_MAP.bits(), 1 << 2);
    assert_eq!(EmitFlags::NO_TRAILING_SOURCE_MAP.bits(), 1 << 3);
    assert_eq!(EmitFlags::NO_NESTED_SOURCE_MAPS.bits(), 1 << 4);
    assert_eq!(EmitFlags::NO_TOKEN_LEADING_SOURCE_MAPS.bits(), 1 << 5);
    assert_eq!(EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS.bits(), 1 << 6);
    assert_eq!(EmitFlags::NO_LEADING_COMMENTS.bits(), 1 << 7);
    assert_eq!(EmitFlags::NO_TRAILING_COMMENTS.bits(), 1 << 8);
    assert_eq!(EmitFlags::NO_NESTED_COMMENTS.bits(), 1 << 9);
    assert_eq!(EmitFlags::HELPER_NAME.bits(), 1 << 10);
    assert_eq!(EmitFlags::EXPORT_NAME.bits(), 1 << 11);
    assert_eq!(EmitFlags::LOCAL_NAME.bits(), 1 << 12);
    assert_eq!(EmitFlags::INDENTED.bits(), 1 << 13);
    assert_eq!(EmitFlags::NO_INDENTATION.bits(), 1 << 14);
    assert_eq!(EmitFlags::REUSE_TEMP_VARIABLE_SCOPE.bits(), 1 << 15);
    assert_eq!(EmitFlags::CUSTOM_PROLOGUE.bits(), 1 << 16);
    assert_eq!(EmitFlags::NO_ASCII_ESCAPING.bits(), 1 << 17);
    assert_eq!(EmitFlags::EXTERNAL_HELPERS.bits(), 1 << 18);
    assert_eq!(EmitFlags::START_ON_NEW_LINE.bits(), 1 << 19);
    assert_eq!(EmitFlags::INDIRECT_CALL.bits(), 1 << 20);
    assert_eq!(EmitFlags::ASYNC_FUNCTION_BODY.bits(), 1 << 21);
    assert_eq!(EmitFlags::NO_LEXICAL_ARGUMENTS.bits(), 1 << 22);
    assert_eq!(EmitFlags::TRANSFORM_PRIVATE_STATIC_ELEMENTS.bits(), 1 << 23);
}

// Go: internal/printer/emitflags.go (composite flags)
#[test]
fn composite_flags_match_go() {
    assert_eq!(EmitFlags::NONE.bits(), 0);
    assert_eq!(
        EmitFlags::NO_SOURCE_MAP,
        EmitFlags::NO_LEADING_SOURCE_MAP | EmitFlags::NO_TRAILING_SOURCE_MAP
    );
    assert_eq!(
        EmitFlags::NO_TOKEN_SOURCE_MAPS,
        EmitFlags::NO_TOKEN_LEADING_SOURCE_MAPS | EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS
    );
    assert_eq!(
        EmitFlags::NO_COMMENTS,
        EmitFlags::NO_LEADING_COMMENTS | EmitFlags::NO_TRAILING_COMMENTS
    );
}
