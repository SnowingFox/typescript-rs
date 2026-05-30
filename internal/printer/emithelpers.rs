//! Port of Go `internal/printer/helpers.go`: the [`EmitHelper`] model and the
//! TS runtime-helper definitions (`__setFunctionName`, `__awaiter`, `__rest`,
//! `Object.assign`-style `__assign`, the disposable-resource helpers, the ES
//! module helpers, …).
//!
//! A helper is a small, self-contained `var __name = …;` runtime function whose
//! text is emitted once at the top of a source file when a transform
//! [`requests`](crate::EmitContext::request_emit_helper) it. Helpers are static
//! singletons compared by identity (here: by their unique [`name`](EmitHelper::name)).

/// A reusable runtime helper emitted into a module's prologue on demand.
///
/// Side effects: none (pure value type; the statics are `'static`).
// Go: internal/printer/helpers.go:EmitHelper
#[derive(Debug)]
pub struct EmitHelper {
    /// A unique name identifying this helper (used for de-duplication).
    pub name: &'static str,
    /// Whether the helper MUST be emitted in the current (inner) scope rather
    /// than hoisted to the module prologue. All TS library helpers are unscoped.
    pub scoped: bool,
    /// ES3-compatible raw script text of the helper definition.
    pub text: &'static str,
    /// Helpers with a lower priority value are emitted earlier; `None` sorts
    /// after any explicit priority.
    pub priority: Option<i32>,
    /// Other helpers this helper depends on (emitted/requested first).
    pub dependencies: &'static [&'static EmitHelper],
    /// The name to import when using `--importHelpers` (e.g. `__setFunctionName`).
    pub import_name: &'static str,
}

impl EmitHelper {
    /// Reports whether two helper references are the same definition (identity
    /// by unique name).
    ///
    /// Side effects: none.
    pub fn is(&self, other: &EmitHelper) -> bool {
        self.name == other.name
    }
}

/// Orders two helpers for prologue emit: lower [`priority`](EmitHelper::priority)
/// values sort earlier, and a `None` priority sorts after any explicit priority.
/// Identical helpers (and equal priorities) compare equal, so a stable sort
/// preserves request order among them.
///
/// Side effects: none.
// Go: internal/printer/helpers.go:compareEmitHelpers
pub fn compare_emit_helpers(x: &EmitHelper, y: &EmitHelper) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    if x.is(y) || x.priority == y.priority {
        return Ordering::Equal;
    }
    match (x.priority, y.priority) {
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
        (Some(a), Some(b)) => a.cmp(&b),
    }
}

/// `https://tc39.es/ecma262/#sec-setfunctionname` — sets a function's `.name`.
///
/// Side effects: none (static definition).
// Go: internal/printer/helpers.go:setFunctionNameHelper
pub static SET_FUNCTION_NAME_HELPER: EmitHelper = EmitHelper {
    name: "typescript:setFunctionName",
    import_name: "__setFunctionName",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __setFunctionName = (this && this.__setFunctionName) || function (f, name, prefix) {
    if (typeof name === "symbol") name = name.description ? "[".concat(name.description, "]") : "";
    return Object.defineProperty(f, "name", { configurable: true, value: prefix ? "".concat(prefix, " ", name) : name });
};"#,
};

// --- Defined-but-not-yet-consumed helpers (their transform consumers land in
// 6d-3+: async/forawait/using/objectrestspread-rest/moduletransforms). Texts are
// verbatim ports of Go `internal/printer/helpers.go`. ---

/// ES2017 `__awaiter` — runs an async function body as a generator driven by a
/// promise. Consumer: `estransforms/async.rs` (not yet ported).
// Go: internal/printer/helpers.go:awaiterHelper
pub static AWAITER_HELPER: EmitHelper = EmitHelper {
    name: "typescript:awaiter",
    import_name: "__awaiter",
    scoped: false,
    priority: Some(5),
    dependencies: &[],
    text: r#"var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};"#,
};

/// ES2018 `__rest` — object rest-binding destructuring. Consumer:
/// `estransforms/objectrestspread.rs` rest binding (not yet ported).
// Go: internal/printer/helpers.go:restHelper
pub static REST_HELPER: EmitHelper = EmitHelper {
    name: "typescript:rest",
    import_name: "__rest",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __rest = (this && this.__rest) || function (s, e) {
    var t = {};
    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)
        t[p] = s[p];
    if (s != null && typeof Object.getOwnPropertySymbols === "function")
        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {
            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))
                t[p[i]] = s[p[i]];
        }
    return t;
};"#,
};

/// ESNext `__addDisposableResource` — registers a `using` resource for disposal.
/// Consumer: `estransforms/using.rs` (not yet ported).
// Go: internal/printer/helpers.go:addDisposableResourceHelper
pub static ADD_DISPOSABLE_RESOURCE_HELPER: EmitHelper = EmitHelper {
    name: "typescript:addDisposableResource",
    import_name: "__addDisposableResource",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __addDisposableResource = (this && this.__addDisposableResource) || function (env, value, async) {
    if (value !== null && value !== void 0) {
        if (typeof value !== "object" && typeof value !== "function") throw new TypeError("Object expected.");
        var dispose, inner;
        if (async) {
            if (!Symbol.asyncDispose) throw new TypeError("Symbol.asyncDispose is not defined.");
            dispose = value[Symbol.asyncDispose];
        }
        if (dispose === void 0) {
            if (!Symbol.dispose) throw new TypeError("Symbol.dispose is not defined.");
            dispose = value[Symbol.dispose];
            if (async) inner = dispose;
        }
        if (typeof dispose !== "function") throw new TypeError("Object not disposable.");
        if (inner) dispose = function() { try { inner.call(this); } catch (e) { return Promise.reject(e); } };
        env.stack.push({ value: value, dispose: dispose, async: async });
    }
    else if (async) {
        env.stack.push({ async: true });
    }
    return value;
};"#,
};

/// ESNext `__disposeResources` — disposes registered `using` resources.
/// Consumer: `estransforms/using.rs` (not yet ported).
// Go: internal/printer/helpers.go:disposeResourcesHelper
pub static DISPOSE_RESOURCES_HELPER: EmitHelper = EmitHelper {
    name: "typescript:disposeResources",
    import_name: "__disposeResources",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __disposeResources = (this && this.__disposeResources) || (function (SuppressedError) {
    return function (env) {
        function fail(e) {
            env.error = env.hasError ? new SuppressedError(e, env.error, "An error was suppressed during disposal.") : e;
            env.hasError = true;
        }
        var r, s = 0;
        function next() {
            while (r = env.stack.pop()) {
                try {
                    if (!r.async && s === 1) return s = 0, env.stack.push(r), Promise.resolve().then(next);
                    if (r.dispose) {
                        var result = r.dispose.call(r.value);
                        if (r.async) return s |= 2, Promise.resolve(result).then(next, function(e) { fail(e); return next(); });
                    }
                    else s |= 1;
                }
                catch (e) {
                    fail(e);
                }
            }
            if (s === 1) return env.hasError ? Promise.reject(env.error) : Promise.resolve();
            if (env.hasError) throw env.error;
        }
        return next();
    };
})(typeof SuppressedError === "function" ? SuppressedError : function (error, suppressed, message) {
    var e = new Error(message);
    return e.name = "SuppressedError", e.error = error, e.suppressed = suppressed, e;
});"#,
};

/// `__createBinding` — re-exports a binding (CJS interop). Dependency of
/// `__importStar`/`__exportStar`. Consumer: `moduletransforms` (not yet ported).
// Go: internal/printer/helpers.go:createBindingHelper
pub static CREATE_BINDING_HELPER: EmitHelper = EmitHelper {
    name: "typescript:commonjscreatebinding",
    import_name: "__createBinding",
    scoped: false,
    priority: Some(1),
    dependencies: &[],
    text: r#"var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));"#,
};

/// `__setModuleDefault` — sets a synthetic `default` export. Dependency of
/// `__importStar`. Consumer: `moduletransforms` (not yet ported).
// Go: internal/printer/helpers.go:setModuleDefaultHelper
pub static SET_MODULE_DEFAULT_HELPER: EmitHelper = EmitHelper {
    name: "typescript:commonjscreatevalue",
    import_name: "__setModuleDefault",
    scoped: false,
    priority: Some(1),
    dependencies: &[],
    text: r#"var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});"#,
};

/// `__importStar` — namespace import interop. Depends on `__createBinding` and
/// `__setModuleDefault`. Consumer: `moduletransforms` (not yet ported).
// Go: internal/printer/helpers.go:importStarHelper
pub static IMPORT_STAR_HELPER: EmitHelper = EmitHelper {
    name: "typescript:commonjsimportstar",
    import_name: "__importStar",
    scoped: false,
    priority: Some(2),
    dependencies: &[&CREATE_BINDING_HELPER, &SET_MODULE_DEFAULT_HELPER],
    text: r#"var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();"#,
};

/// `__importDefault` — default import interop. Consumer: `moduletransforms`
/// (not yet ported).
// Go: internal/printer/helpers.go:importDefaultHelper
pub static IMPORT_DEFAULT_HELPER: EmitHelper = EmitHelper {
    name: "typescript:commonjsimportdefault",
    import_name: "__importDefault",
    scoped: false,
    priority: None,
    dependencies: &[],
    text: r#"var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};"#,
};

/// `__exportStar` — `export * from` interop. Depends on `__createBinding`.
/// Consumer: `moduletransforms` (not yet ported).
// Go: internal/printer/helpers.go:exportStarHelper
pub static EXPORT_STAR_HELPER: EmitHelper = EmitHelper {
    name: "typescript:export-star",
    import_name: "__exportStar",
    scoped: false,
    priority: Some(2),
    dependencies: &[&CREATE_BINDING_HELPER],
    text: r#"var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};"#,
};

#[cfg(test)]
#[path = "emithelpers_test.rs"]
mod tests;
