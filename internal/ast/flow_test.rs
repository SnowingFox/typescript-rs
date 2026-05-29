use super::*;

// Go: internal/ast/flow.go:FlowFlags (base bit positions)
#[test]
fn flow_flags_base_bits() {
    assert_eq!(FlowFlags::UNREACHABLE.bits(), 1 << 0);
    assert_eq!(FlowFlags::START.bits(), 1 << 1);
    assert_eq!(FlowFlags::BRANCH_LABEL.bits(), 1 << 2);
    assert_eq!(FlowFlags::LOOP_LABEL.bits(), 1 << 3);
    assert_eq!(FlowFlags::SWITCH_CLAUSE.bits(), 1 << 7);
    assert_eq!(FlowFlags::SHARED.bits(), 1 << 12);
}

// Go: internal/ast/flow.go:FlowFlags (unions — values from Go literals)
#[test]
fn flow_flags_unions() {
    assert_eq!(
        FlowFlags::LABEL,
        FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL
    );
    assert_eq!(
        FlowFlags::CONDITION,
        FlowFlags::TRUE_CONDITION | FlowFlags::FALSE_CONDITION
    );
}

// Go: internal/ast/flow.go:FlowSwitchClauseData.IsEmpty
#[test]
fn flow_switch_clause_is_empty() {
    let empty = FlowSwitchClauseData {
        switch_statement: None,
        clause_start: 3,
        clause_end: 3,
    };
    let non_empty = FlowSwitchClauseData {
        switch_statement: None,
        clause_start: 1,
        clause_end: 4,
    };
    assert!(empty.is_empty());
    assert!(!non_empty.is_empty());
}
