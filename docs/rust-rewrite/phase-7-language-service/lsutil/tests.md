# ls/lsutil: test inventory (tests.md)

> The crate starts at 0 tests. Go has only two test files
> (`utilities_test.go` → `TestProbablyUsesSemicolons`, and
> `userpreferences_test.go`), both for functions that are DEFER'd in this round
> (`ProbablyUsesSemicolons`; the full `UserPreferences` (un)marshaling). Per
> PORTING §8.7/§8.10 ("more tests than Go, never fewer; every reachable function
> gets a behavior test"), this round adds behavior-level tests for every ported
> helper.

Final count: **40 unit tests + 25 doctests**, all green
(`cargo test -p tsgo_ls_lsutil`).

## `asi_test.rs` (9 unit) — Go: `asi.go`

| Rust test | Go anchor / behavior |
|---|---|
| `semicolon_or_asi_covers_all_terminated_statements` | `SyntaxRequiresTrailingSemicolonOrASI` — all 15 kinds true |
| `semicolon_or_asi_rejects_others` | same — non-members false |
| `comma_or_semicolon_or_asi_covers_type_member_signatures` | `SyntaxRequiresTrailingCommaOrSemicolonOrASI` — 5 signature kinds |
| `comma_or_semicolon_or_asi_rejects_others` | same — non-members false |
| `function_block_or_semicolon_or_asi_covers_function_like` | `SyntaxRequiresTrailingFunctionBlockOrSemicolonOrASI` — 5 kinds |
| `function_block_or_semicolon_or_asi_rejects_others` | same — non-members false |
| `module_block_or_semicolon_or_asi_only_module_declaration` | `SyntaxRequiresTrailingModuleBlockOrSemicolonOrASI` |
| `may_be_asi_candidate_is_union_of_the_four` | `SyntaxMayBeASICandidate` — one rep per class |
| `may_be_asi_candidate_rejects_non_candidates` | same — non-candidates false |

## `children_test.rs` (11 unit) — Go: `children.go`

| Rust test | Go anchor / behavior |
|---|---|
| `assert_has_real_position_ok_for_parsed_node` | `AssertHasRealPosition` — real position OK |
| `assert_has_real_position_panics_on_synthesized` | same — `#[should_panic]` on synthesized (`pos = -1`) |
| `last_visited_child_is_declaration_list` | `GetLastVisitedChild` — `let a = 1;` → `VariableDeclarationList` |
| `last_visited_child_none_for_leaf_token` | same — identifier has no visited child |
| `last_child_is_trailing_semicolon` | `GetLastChild` — trailing `;` synthesized as last child |
| `last_child_without_trailing_token_is_last_visited` | same — no `;` → last visited child |
| `last_token_descends_to_semicolon` | `GetLastToken` — descends to `;` |
| `last_token_none_for_identifier` | same — identifier → `None` |
| `first_token_is_let_keyword` | `GetFirstToken` — first token is `let` |
| `first_token_none_for_identifier` | same — identifier → `None` |
| `last_token_is_cached_stable` | `GetOrCreateToken` — repeated queries return the same node id |

## `utilities_test.rs` (10 unit) — Go: `utilities.go`

| Rust test | Go anchor / behavior |
|---|---|
| `is_non_contextual_keyword_accepts_reserved_keywords` | `IsNonContextualKeyword` — reserved keywords true |
| `is_non_contextual_keyword_rejects_contextual_and_non_keywords` | same — contextual + non-keywords false |
| `quote_preference_from_single_quoted_literal` | `QuotePreferenceFromString` — `'a'` → Single |
| `quote_preference_from_double_quoted_literal` | same — `"a"` → Double |
| `module_specifier_camel_cases_across_invalid_chars` | `ModuleSpecifierToValidIdentifier` — `./foo-bar` → `fooBar` |
| `module_specifier_force_capitalize_uppercases_first` | same — force-capitalize → `FooBar` |
| `module_specifier_strips_extension_and_index` | same — `./foo/index.ts` → `foo`, `./bar.d.ts` → `bar` |
| `module_specifier_keyword_collision_gets_underscore` | same — `./if` → `_if` |
| `module_specifier_all_invalid_chars_becomes_underscore` | same — `./---` → `_` |
| `module_symbol_strips_quotes_then_converts` | `ModuleSymbolToValidIdentifier` — `"./foo-bar"` → `fooBar`/`FooBar` |

## `userpreferences_test.rs` (2 unit) — Go: `userpreferences.go`

| Rust test | Go anchor / behavior |
|---|---|
| `quote_preference_wire_values` | `QuotePreference` constants — `""`/`auto`/`double`/`single` |
| `quote_preference_default_is_unknown` | zero value is `Unknown` |

> Note: Go's `userpreferences_test.go` (`TestUserPreferencesRoundtrip`,
> `...Serialize`, `...ParseUnstable`, `...ParseATA`) targets the full
> reflection-based marshaling, which is DEFER'd; those tests move with that port.

## `symbol_display_test.rs` (8 unit) — Go: `symbol_display.go`

| Rust test | Go anchor / behavior |
|---|---|
| `script_element_kind_discriminants_match_iota` | `ScriptElementKind` — iota 0/1/2/5/15/22/38 |
| `script_element_kind_default_is_unknown` | zero value is `Unknown` |
| `modifier_bit_values_start_at_bit_one` | `ScriptElementKindModifier` — `Public` = bit 1, `Cjs` = bit 21 |
| `strings_returns_names_in_table_order` | `.Strings()` — table order, not insertion order |
| `strings_uses_dotted_names_for_file_extensions` | same — `.d.ts`/`.tsx` |
| `strings_empty_for_none` | same — empty → `[]` |
| `strings_maps_exported_and_ambient_to_keywords` | same — `export`/`declare` |
| `file_extension_modifiers_contains_all_extension_flags` | `FileExtensionKindModifiers` — all 12 ext flags, no others |

## Doctests (25)

Every public item carries a runnable `# Examples` doctest (asi: 5; children:
`SourceFile`/`new`/`root`/`text`/`arena` + `assert_has_real_position` +
`get_last_visited_child`/`get_last_child`/`get_last_token`/`get_first_token` = 10;
utilities: 4; userpreferences: 2; symbol_display: 4) — 25 total.
