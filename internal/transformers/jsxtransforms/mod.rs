//! Port of Go `internal/transformers/jsxtransforms/jsx.go`: lowers JSX syntax to
//! factory calls.
//!
//! Round 6f lands the **classic runtime** (`React.createElement`) element/
//! fragment lowering. The **automatic runtime** (`jsx`/`jsxs`/`jsxDEV` +
//! implicit-import injection) and custom `@jsxFactory`/`@jsxImportSource`
//! pragmas are deferred — see [`jsx`] module docs.

pub mod jsx;
