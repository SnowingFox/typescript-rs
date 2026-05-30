use super::*;

// Go: internal/module/types.go:NodeResolutionFeatures (iota bit values)
#[test]
fn node_resolution_features_bit_values() {
    assert_eq!(NodeResolutionFeatures::IMPORTS.bits(), 1);
    assert_eq!(NodeResolutionFeatures::SELF_NAME.bits(), 2);
    assert_eq!(NodeResolutionFeatures::EXPORTS.bits(), 4);
    assert_eq!(NodeResolutionFeatures::EXPORTS_PATTERN_TRAILERS.bits(), 8);
    assert_eq!(NodeResolutionFeatures::IMPORTS_PATTERN_ROOT.bits(), 16);
    assert_eq!(NodeResolutionFeatures::NONE.bits(), 0);
    assert_eq!(NodeResolutionFeatures::ALL.bits(), 31);
    assert_eq!(NodeResolutionFeatures::NODE16_DEFAULT.bits(), 15);
    assert_eq!(NodeResolutionFeatures::NODENEXT_DEFAULT.bits(), 31);
    assert_eq!(NodeResolutionFeatures::BUNDLER_DEFAULT.bits(), 31);
}

// Go: internal/module/types.go:extensions (iota bit values)
#[test]
fn extensions_bit_values() {
    assert_eq!(Extensions::TYPE_SCRIPT.bits(), 1);
    assert_eq!(Extensions::JAVA_SCRIPT.bits(), 2);
    assert_eq!(Extensions::DECLARATION.bits(), 4);
    assert_eq!(Extensions::JSON.bits(), 8);
    assert_eq!(Extensions::IMPLEMENTATION_FILES.bits(), 3);
}

// Go: internal/module/types.go:extensions.String
#[test]
fn extensions_string() {
    assert_eq!(Extensions::empty().to_string(), "");
    assert_eq!(Extensions::TYPE_SCRIPT.to_string(), "TypeScript");
    assert_eq!(Extensions::JAVA_SCRIPT.to_string(), "JavaScript");
    assert_eq!(Extensions::DECLARATION.to_string(), "Declaration");
    assert_eq!(Extensions::JSON.to_string(), "JSON");
    assert_eq!(
        (Extensions::TYPE_SCRIPT
            | Extensions::JAVA_SCRIPT
            | Extensions::DECLARATION
            | Extensions::JSON)
            .to_string(),
        "TypeScript, JavaScript, Declaration, JSON"
    );
}

// Go: internal/module/types.go:extensions.Array
#[test]
fn extensions_array() {
    assert_eq!(
        Extensions::TYPE_SCRIPT.to_array(),
        vec![".ts", ".tsx", ".mts", ".cts"]
    );
    assert_eq!(
        Extensions::JAVA_SCRIPT.to_array(),
        vec![".js", ".jsx", ".mjs", ".cjs"]
    );
    assert_eq!(
        Extensions::DECLARATION.to_array(),
        vec![".d.ts", ".d.cts", ".d.mts"]
    );
    assert_eq!(Extensions::JSON.to_array(), vec![".json"]);
    assert_eq!(
        Extensions::IMPLEMENTATION_FILES.to_array(),
        vec![".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]
    );
}

// Go: internal/module/types.go:PackageId.PackageName
#[test]
fn package_id_package_name() {
    let with_sub = PackageId {
        name: "pkg".into(),
        sub_module_name: "lib/sub".into(),
        version: "1.0.0".into(),
        peer_dependencies: String::new(),
    };
    assert_eq!(with_sub.package_name(), "pkg/lib/sub");

    let no_sub = PackageId {
        name: "pkg".into(),
        sub_module_name: String::new(),
        version: "1.0.0".into(),
        peer_dependencies: String::new(),
    };
    assert_eq!(no_sub.package_name(), "pkg");
}

// Go: internal/module/types.go:PackageId.String
#[test]
fn package_id_display() {
    let id = PackageId {
        name: "@scope/pkg".into(),
        sub_module_name: "sub".into(),
        version: "2.1.0".into(),
        peer_dependencies: "+peer@1.0.0".into(),
    };
    assert_eq!(id.to_string(), "@scope/pkg/sub@2.1.0+peer@1.0.0");
}

// Go: internal/module/types.go:ResolvedModule.IsResolved
#[test]
fn resolved_module_is_resolved() {
    let mut r = ResolvedModule::default();
    assert!(!r.is_resolved());
    r.resolved_file_name = "/repo/a.ts".into();
    assert!(r.is_resolved());
}

// Go: internal/module/types.go:ResolvedTypeReferenceDirective.IsResolved
#[test]
fn resolved_type_reference_directive_is_resolved() {
    let mut r = ResolvedTypeReferenceDirective::default();
    assert!(!r.is_resolved());
    r.resolved_file_name = "/repo/a.d.ts".into();
    assert!(r.is_resolved());
}
