use super::*;

// Go: internal/nodebuilder/types.go:Flags (option bit positions, values from Go literals)
#[test]
fn flags_bit_values() {
    assert_eq!(Flags::NO_TRUNCATION.bits(), 1 << 0);
    assert_eq!(Flags::OMIT_PARAMETER_MODIFIERS.bits(), 1 << 13);
    assert_eq!(
        Flags::USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE.bits(),
        1 << 14
    );
    assert_eq!(Flags::OMIT_THIS_PARAMETER.bits(), 1 << 25);
    assert_eq!(Flags::ALLOW_NODE_MODULES_RELATIVE_PATHS.bits(), 1 << 26);
    assert_eq!(Flags::WRITE_CALL_STYLE_SIGNATURE.bits(), 1 << 27);
    assert_eq!(
        Flags::USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE.bits(),
        1 << 28
    );
    assert_eq!(Flags::NO_TYPE_REDUCTION.bits(), 1 << 29);
    assert_eq!(Flags::USE_INSTANTIATION_EXPRESSIONS.bits(), 1 << 30);
}

// Go: internal/nodebuilder/types.go:Flags (State bit positions)
#[test]
fn flags_state_bits() {
    assert_eq!(Flags::IN_OBJECT_TYPE_LITERAL.bits(), 1 << 22);
    assert_eq!(Flags::IN_TYPE_ALIAS.bits(), 1 << 23);
    assert_eq!(Flags::IN_INITIAL_ENTITY_NAME.bits(), 1 << 24);
}

// Go: internal/nodebuilder/types.go:FlagsIgnoreErrors (composite membership)
#[test]
fn flags_ignore_errors_composition() {
    assert_eq!(
        Flags::IGNORE_ERRORS,
        Flags::ALLOW_THIS_IN_OBJECT_LITERAL
            | Flags::ALLOW_QUALIFIED_NAME_IN_PLACE_OF_IDENTIFIER
            | Flags::ALLOW_ANONYMOUS_IDENTIFIER
            | Flags::ALLOW_EMPTY_UNION_OR_INTERSECTION
            | Flags::ALLOW_EMPTY_TUPLE
            | Flags::ALLOW_EMPTY_INDEX_INFO_TYPE
            | Flags::ALLOW_NODE_MODULES_RELATIVE_PATHS
    );
    // The error-handling group occupies bits 15..=21 (+ bit 26 above).
    assert_eq!(Flags::ALLOW_THIS_IN_OBJECT_LITERAL.bits(), 1 << 15);
    assert_eq!(
        Flags::ALLOW_QUALIFIED_NAME_IN_PLACE_OF_IDENTIFIER.bits(),
        1 << 16
    );
    assert_eq!(Flags::ALLOW_ANONYMOUS_IDENTIFIER.bits(), 1 << 17);
    assert_eq!(Flags::ALLOW_EMPTY_UNION_OR_INTERSECTION.bits(), 1 << 18);
    assert_eq!(Flags::ALLOW_EMPTY_TUPLE.bits(), 1 << 19);
    assert_eq!(Flags::ALLOW_UNIQUE_ES_SYMBOL_TYPE.bits(), 1 << 20);
    assert_eq!(Flags::ALLOW_EMPTY_INDEX_INFO_TYPE.bits(), 1 << 21);
    // AllowUniqueESSymbolType (bit 20) is intentionally NOT part of IGNORE_ERRORS.
    assert!(!Flags::IGNORE_ERRORS.contains(Flags::ALLOW_UNIQUE_ES_SYMBOL_TYPE));
}

// Go: internal/nodebuilder/types.go:InternalFlags (bit positions)
#[test]
fn internal_flags_bit_values() {
    assert_eq!(InternalFlags::NONE.bits(), 0);
    assert_eq!(InternalFlags::WRITE_COMPUTED_PROPS.bits(), 1 << 0);
    assert_eq!(InternalFlags::NO_SYNTACTIC_PRINTER.bits(), 1 << 1);
    assert_eq!(InternalFlags::DO_NOT_INCLUDE_SYMBOL_CHAIN.bits(), 1 << 2);
    assert_eq!(InternalFlags::ALLOW_UNRESOLVED_NAMES.bits(), 1 << 3);
}

// Go: internal/nodebuilder/types.go:SymbolTracker (object safety + full method surface)
#[test]
fn symbol_tracker_object_safe() {
    #[derive(Default)]
    struct MockTracker {
        track_result: bool,
        calls: u32,
    }

    impl SymbolTracker for MockTracker {
        fn track_symbol(
            &mut self,
            _symbol: SymbolId,
            _enclosing_declaration: Option<NodeId>,
            _meaning: SymbolFlags,
        ) -> bool {
            self.calls += 1;
            self.track_result
        }
        fn report_inaccessible_this_error(&mut self) {
            self.calls += 1;
        }
        fn report_private_in_base_of_class_expression(&mut self, _property_name: &str) {
            self.calls += 1;
        }
        fn report_inaccessible_unique_symbol_error(&mut self) {
            self.calls += 1;
        }
        fn report_cyclic_structure_error(&mut self) {
            self.calls += 1;
        }
        fn report_likely_unsafe_import_required_error(
            &mut self,
            _specifier: &str,
            _symbol_name: &str,
        ) {
            self.calls += 1;
        }
        fn report_truncation_error(&mut self) {
            self.calls += 1;
        }
        fn report_nonlocal_augmentation(
            &mut self,
            _containing_file: NodeId,
            _parent_symbol: SymbolId,
            _augmenting_symbol: SymbolId,
        ) {
            self.calls += 1;
        }
        fn report_non_serializable_property(&mut self, _property_name: &str) {
            self.calls += 1;
        }
        fn report_inference_fallback(&mut self, _node: NodeId) {
            self.calls += 1;
        }
        fn push_error_fallback_node(&mut self, _node: NodeId) {
            self.calls += 1;
        }
        fn pop_error_fallback_node(&mut self) {
            self.calls += 1;
        }
    }

    let mut mock = MockTracker {
        track_result: true,
        calls: 0,
    };
    {
        // The trait must be object-safe; `&mut dyn` only coerces if it is.
        let tracker: &mut dyn SymbolTracker = &mut mock;

        // `track_symbol` returns the tracker-configured value (mirrors Go's bool).
        assert!(tracker.track_symbol(SymbolId(1), Some(NodeId(2)), SymbolFlags::CLASS));
        assert!(tracker.track_symbol(SymbolId(1), None, SymbolFlags::NONE));

        tracker.report_inaccessible_this_error();
        tracker.report_private_in_base_of_class_expression("propertyName");
        tracker.report_inaccessible_unique_symbol_error();
        tracker.report_cyclic_structure_error();
        tracker.report_likely_unsafe_import_required_error("./specifier", "symName");
        tracker.report_truncation_error();
        tracker.report_nonlocal_augmentation(NodeId(3), SymbolId(4), SymbolId(5));
        tracker.report_non_serializable_property("nonSerializable");
        tracker.report_inference_fallback(NodeId(6));
        tracker.push_error_fallback_node(NodeId(7));
        tracker.pop_error_fallback_node();
    }
    // Every method was dispatched through the trait object exactly once.
    assert_eq!(mock.calls, 13);

    // A `Box<dyn SymbolTracker>` proves boxing works; a `false`-returning
    // tracker proves the return value is not hard-coded.
    let mut deny: Box<dyn SymbolTracker> = Box::new(MockTracker::default());
    assert!(!deny.track_symbol(SymbolId(0), None, SymbolFlags::NONE));
}
