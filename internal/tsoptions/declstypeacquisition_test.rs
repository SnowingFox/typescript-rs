use super::*;

use crate::commandlineoption::CommandLineOptionKind;

// Go: internal/tsoptions/declstypeacquisition.go:typeAcquisitionDeclaration
#[test]
fn type_acquisition_declaration_has_element_options() {
    let decl = &*TYPE_ACQUISITION_DECLARATION;
    assert_eq!(decl.name, "typeAcquisition");
    assert_eq!(decl.kind, CommandLineOptionKind::Object);
    let elements = decl.element_options.as_ref().expect("has element options");
    assert_eq!(
        elements.get("enable").map(|o| o.kind),
        Some(CommandLineOptionKind::Boolean)
    );
    assert_eq!(
        elements.get("include").map(|o| o.kind),
        Some(CommandLineOptionKind::List)
    );
}
