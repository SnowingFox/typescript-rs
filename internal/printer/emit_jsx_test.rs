use crate::test_support::check_emit;

// Go: internal/printer/printer_test.go:TestEmit/JsxElement
#[test]
fn jsx_element() {
    let cases = [
        ("<a></a>", "<a></a>;"),
        ("<this></this>", "<this></this>;"),
        ("<a:b></a:b>", "<a:b></a:b>;"),
        ("<a.b></a.b>", "<a.b></a.b>;"),
        ("<a b></a>", "<a b></a>;"),
        ("<a>b</a>", "<a>b</a>;"),
        ("<a>{b}</a>", "<a>{b}</a>;"),
        ("<a><b></b></a>", "<a><b></b></a>;"),
        ("<a><b /></a>", "<a><b /></a>;"),
        ("<a><></></a>", "<a><></></a>;"),
    ];
    for (i, o) in cases {
        check_emit(i, o, true);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/JsxSelfClosingElement
#[test]
fn jsx_self_closing_element() {
    let cases = [
        ("<a />", "<a />;"),
        ("<this />", "<this />;"),
        ("<a:b />", "<a:b />;"),
        ("<a.b />", "<a.b />;"),
        ("<a b/>", "<a b/>;"),
    ];
    for (i, o) in cases {
        check_emit(i, o, true);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/JsxFragment
#[test]
fn jsx_fragment() {
    let cases = [
        ("<></>", "<></>;"),
        ("<>b</>", "<>b</>;"),
        ("<>{b}</>", "<>{b}</>;"),
        ("<><b></b></>", "<><b></b></>;"),
        ("<><b /></>", "<><b /></>;"),
        ("<><></></>", "<><></></>;"),
    ];
    for (i, o) in cases {
        check_emit(i, o, true);
    }
}

// Go: internal/printer/printer_test.go:TestEmit/JsxAttribute + JsxSpreadAttribute
#[test]
fn jsx_attributes() {
    let cases = [
        ("<a b/>", "<a b/>;"),
        ("<a b:c/>", "<a b:c/>;"),
        ("<a b=\"c\"/>", "<a b=\"c\"/>;"),
        ("<a b={c}/>", "<a b={c}/>;"),
        ("<a b=<c></c>/>", "<a b=<c></c>/>;"),
        ("<a b=<c />/>", "<a b=<c />/>;"),
        ("<a b=<></>/>", "<a b=<></>/>;"),
        ("<a {...b}/>", "<a {...b}/>;"),
    ];
    for (i, o) in cases {
        check_emit(i, o, true);
    }
}
