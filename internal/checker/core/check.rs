//! Expression/statement checking and diagnostics.
//!
//! Ports the reachable core of Go's `checker.go` `checkSourceFile` →
//! `checkStatement` → `checkExpression` recursion plus `getDiagnostics`. 4g
//! covers literals, identifiers, property/element access, a minimal call
//! resolution, and "Cannot find name" reporting; the type of each expression
//! feeds the 4f flow engine.
//!
//! DEFER(phase-4-checker-4h+): JSX, grammar checks, the full statement/
//! expression checking surface (assignments, control-flow statements, classes,
//! contextual typing, unused checks), and the node builder.

use tsgo_ast::{Kind, NodeData, NodeId, SymbolFlags};
use tsgo_diagnostics::{Category, Message};

use super::declared_types::{get_apparent_type, get_type_of_property_of_type, get_type_of_symbol};
use super::mapper::TypeMapper;
use super::program::BoundProgram;
use super::signatures::SignatureId;
use super::symbols::resolve_name;
use super::types::{LiteralValue, TypeFlags, TypeId};
use super::Checker;

/// A type-checking diagnostic produced while checking a source file.
///
/// A minimal stand-in for Go's `ast.Diagnostic` (which also carries the file,
/// related information, and message chains); 4g records just the span, code,
/// category, and localized text.
///
/// DEFER(phase-4-checker-4j): message chains + related information + the owning
/// `SourceFile`. blocked-by: the real `ast.Diagnostic`/`DiagnosticsCollection`
/// (program-level, P6) and the node builder (4j).
// Go: internal/ast/diagnostic.go:Diagnostic (subset)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The numeric diagnostic code (e.g. `2304`).
    pub code: i32,
    /// The diagnostic category (error/warning/...).
    pub category: Category,
    /// The localized, argument-substituted message text.
    pub message: String,
    /// Start position in the source text.
    pub start: i32,
    /// Length of the flagged span.
    pub length: i32,
}

impl Checker {
    /// Computes the type of an expression `node` (Go's `checkExpression`).
    ///
    /// 4g handles literals, identifiers (resolved + flow-narrowed), property and
    /// element access, and calls; unhandled kinds yield the error type.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, n: tsgo_ast::NodeId) {
    /// let _ = c.check_expression(p, n);
    /// # }
    /// ```
    ///
    /// Side effects: may record diagnostics and allocate types.
    // Go: internal/checker/checker.go:Checker.checkExpression(7521)/checkExpressionWorker(7699)
    pub fn check_expression(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        match program.arena().kind(node) {
            Kind::Identifier => self.check_identifier(program, node),
            Kind::StringLiteral => {
                let text = program.arena().text(node).to_string();
                self.new_literal_type(TypeFlags::STRING_LITERAL, LiteralValue::String(text), None)
            }
            Kind::NumericLiteral => {
                let value = tsgo_jsnum::from_string(program.arena().text(node));
                self.new_literal_type(TypeFlags::NUMBER_LITERAL, LiteralValue::Number(value), None)
            }
            Kind::TrueKeyword => self.true_type,
            Kind::FalseKeyword => self.false_type,
            Kind::NullKeyword => self.null_type,
            Kind::PropertyAccessExpression => self.check_property_access(program, node),
            Kind::ElementAccessExpression => self.check_element_access(program, node),
            Kind::JsxSelfClosingElement => self.check_jsx_self_closing_element(program, node),
            Kind::JsxElement => self.check_jsx_element(program, node),
            Kind::JsxFragment => self.check_jsx_fragment(program, node),
            // DEFER(phase-4-checker-4h+): remaining expression kinds are added in
            // later 4g slices / sub-phases.
            _ => self.error_type,
        }
    }

    // Resolves an identifier reference to its (flow-narrowed) value type.
    // Go: internal/checker/checker.go:Checker.checkIdentifier(10999)
    fn check_identifier(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let name = program.arena().text(node).to_string();
        match resolve_name(program, node, &name, SymbolFlags::VALUE, false, None) {
            None => {
                self.error(
                    program,
                    node,
                    &tsgo_diagnostics::CANNOT_FIND_NAME_0,
                    &[name.as_str()],
                );
                self.error_type
            }
            Some(symbol) => {
                let declared = get_type_of_symbol(self, program, symbol, None);
                self.get_flow_type_of_reference(program, node, declared)
            }
        }
    }

    // Checks a property access `obj.name`, returning the property's type.
    // Go: internal/checker/checker.go:Checker.checkPropertyAccessExpression
    fn check_property_access(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, name_node) = match program.arena().data(node) {
            NodeData::PropertyAccessExpression(d) => (d.expression, d.name),
            _ => return self.error_type,
        };
        let object_type = self.check_expression(program, expr);
        let name = program.arena().text(name_node).to_string();
        match get_type_of_property_of_type(self, program, object_type, &name) {
            Some(t) => t,
            None => {
                let type_str = self.type_to_string(object_type);
                self.error(
                    program,
                    name_node,
                    &tsgo_diagnostics::PROPERTY_0_DOES_NOT_EXIST_ON_TYPE_1,
                    &[name.as_str(), type_str.as_str()],
                );
                self.error_type
            }
        }
    }

    // Checks an element access `obj[index]`. 4g handles a string-literal index
    // as a property lookup; other index kinds are deferred.
    // Go: internal/checker/checker.go:Checker.checkIndexedAccess
    fn check_element_access(&mut self, program: &dyn BoundProgram, node: NodeId) -> TypeId {
        let (expr, arg) = match program.arena().data(node) {
            NodeData::ElementAccessExpression(d) => (d.expression, d.argument_expression),
            _ => return self.error_type,
        };
        let object_type = self.check_expression(program, expr);
        if program.arena().kind(arg) == Kind::StringLiteral {
            let name = program.arena().text(arg).to_string();
            if let Some(t) = get_type_of_property_of_type(self, program, object_type, &name) {
                return t;
            }
        }
        // DEFER(phase-4-checker-4h+): numeric/computed indices and index
        // signatures. blocked-by: index-signature resolution + apparent type.
        self.error_type
    }

    /// Returns the call signatures of `t` (Go's `getSignaturesOfType` for the
    /// call kind), resolving through a type reference's target.
    ///
    /// DEFER(phase-4-checker-4h+): construct signatures, union/intersection
    /// signature merging, and apparent-type signatures from primitives.
    /// blocked-by: lib globals (P6) + interface call-signature collection.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::Checker;
    /// let c = Checker::new();
    /// assert!(c.get_signatures_of_type(c.string_type()).is_empty());
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.getSignaturesOfType
    pub fn get_signatures_of_type(&self, t: TypeId) -> Vec<SignatureId> {
        let apparent = get_apparent_type(self, t);
        let Some(obj) = self.get_type(apparent).as_object() else {
            return Vec::new();
        };
        match obj.target {
            Some(target) => self
                .get_type(target)
                .as_object()
                .map(|o| o.call_signatures.clone())
                .unwrap_or_default(),
            None => obj.call_signatures.clone(),
        }
    }

    /// Resolves the return type of calling `signature` with `argument_types`,
    /// where `parameter_types` are the signature's parameter types.
    ///
    /// For a non-generic signature this is its declared return type; for a
    /// generic one the type parameters are inferred from the arguments (4e
    /// `infer_type_arguments`) and the return type is instantiated.
    ///
    /// DEFER(phase-4-checker-4h+): overload resolution, arg-count/arg-type
    /// diagnostics, contextual typing, and wiring a `CallExpression` through a
    /// bound program. blocked-by: a callable type built from a function/method
    /// declaration (interface call-signature collection in declared types).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker, Signature, SignatureFlags};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P) {
    /// let num = c.number_type();
    /// let mut sig = Signature::new(SignatureFlags::NONE);
    /// sig.resolved_return_type = Some(num);
    /// let sid = c.new_signature(sig);
    /// assert_eq!(c.get_return_type_of_call(p, sid, &[], &[]), num);
    /// # }
    /// ```
    ///
    /// Side effects: may infer types and allocate an instantiated signature.
    // Go: internal/checker/checker.go:Checker.getReturnTypeOfSignature + inference.go
    pub fn get_return_type_of_call(
        &mut self,
        program: &dyn BoundProgram,
        signature: SignatureId,
        parameter_types: &[TypeId],
        argument_types: &[TypeId],
    ) -> TypeId {
        let type_parameters = self.signature(signature).type_parameters.clone();
        if type_parameters.is_empty() {
            return self
                .signature(signature)
                .resolved_return_type
                .unwrap_or(self.error_type);
        }
        // `infer_types(source, target)` collects candidates into the target's
        // type-parameter slots, so arguments are the sources, parameters the
        // targets.
        let inferred =
            self.infer_type_arguments(program, &type_parameters, argument_types, parameter_types);
        let mapper = TypeMapper::Array {
            sources: type_parameters,
            targets: inferred,
        };
        let instantiated = self.instantiate_signature(signature, &mapper);
        self.signature(instantiated)
            .resolved_return_type
            .unwrap_or(self.error_type)
    }

    /// Type-checks a whole source file, recording diagnostics on the checker
    /// (Go's `checkSourceFile`).
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &mut Checker, p: &P, file: tsgo_ast::NodeId) {
    /// c.check_source_file(p, file);
    /// # }
    /// ```
    ///
    /// Side effects: records diagnostics and allocates types.
    // Go: internal/checker/checker.go:Checker.checkSourceFile(2176)
    pub fn check_source_file(&mut self, program: &dyn BoundProgram, file: NodeId) {
        let statements = match program.arena().data(file) {
            NodeData::SourceFile(d) => d.statements.nodes.clone(),
            _ => return,
        };
        for statement in statements {
            self.check_statement(program, statement);
        }
    }

    // Checks a single statement (4g: expression statements drive checking).
    //
    // DEFER(phase-4-checker-4h+): declarations, control-flow statements, classes,
    // and the rest of the statement surface.
    // Go: internal/checker/checker.go:Checker.checkSourceElement
    fn check_statement(&mut self, program: &dyn BoundProgram, node: NodeId) {
        if let NodeData::ExpressionStatement(d) = program.arena().data(node) {
            let expr = d.expression;
            self.check_expression(program, expr);
        }
    }

    /// Returns the diagnostics recorded for `file` (Go's `getDiagnostics`).
    ///
    /// In 4g the stub program holds a single file, so all recorded diagnostics
    /// are returned. Call [`Checker::check_source_file`] first.
    ///
    /// # Examples
    /// ```
    /// use tsgo_checker::{BoundProgram, Checker};
    /// # fn demo<P: BoundProgram>(c: &Checker, file: tsgo_ast::NodeId) {
    /// let _ = c.get_diagnostics(file);
    /// # }
    /// ```
    ///
    /// Side effects: none (pure).
    // Go: internal/checker/checker.go:Checker.getDiagnostics(13865)
    pub fn get_diagnostics(&self, file: NodeId) -> &[Diagnostic] {
        let _ = file;
        &self.diagnostics
    }

    // Records a diagnostic at `node` from `message` with `args` substituted.
    // Go: internal/checker/checker.go:Checker.error(13893)
    pub(crate) fn error(
        &mut self,
        program: &dyn BoundProgram,
        node: NodeId,
        message: &'static Message,
        args: &[&str],
    ) {
        let loc = program.arena().loc(node);
        self.diagnostics.push(Diagnostic {
            code: message.code(),
            category: message.category(),
            message: tsgo_diagnostics::format(&message.to_string(), args),
            start: loc.pos(),
            length: loc.end() - loc.pos(),
        });
    }
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
