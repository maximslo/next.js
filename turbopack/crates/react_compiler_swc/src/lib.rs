// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is licensed under the MIT license found in the
// LICENSE file in the root directory of this source tree.

pub mod convert_ast;
pub mod convert_ast_reverse;
pub mod convert_scope;
pub mod diagnostics;
pub mod prefilter;

use convert_ast::convert_module_with_source_type;
use convert_ast_reverse::convert_program_to_swc_with_source;
use convert_scope::build_scope_info;
use diagnostics::{DiagnosticMessage, compile_result_to_diagnostics};
use prefilter::has_react_like_functions;
use react_compiler::entrypoint::{compile_result::LoggerEvent, plugin_options::PluginOptions};

/// Result of compiling a program via the SWC frontend.
pub struct TransformResult {
    /// The compiled program as an SWC Module (None if no changes needed).
    pub module: Option<swc_core::ecma::ast::Module>,
    /// Comments extracted from the compiled AST (for use with `emit_with_comments`).
    pub comments: Option<swc_core::common::comments::SingleThreadedComments>,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub events: Vec<LoggerEvent>,
}

/// Result of linting a program via the SWC frontend.
pub struct LintResult {
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Primary transform API — accepts pre-parsed SWC Module.
pub fn transform(
    module: &swc_core::ecma::ast::Module,
    source_text: &str,
    options: PluginOptions,
) -> TransformResult {
    if options.compilation_mode != "all" && !has_react_like_functions(module) {
        return TransformResult {
            module: None,
            comments: None,
            diagnostics: vec![],
            events: vec![],
        };
    }

    // Detect source type from pragma. The @script pragma indicates
    // CommonJS (script) mode, which affects how imports are emitted.
    let source_type = if source_text
        .lines()
        .next()
        .map_or(false, |line| line.contains("@script"))
    {
        react_compiler_ast::SourceType::Script
    } else {
        react_compiler_ast::SourceType::Module
    };
    let file = convert_module_with_source_type(module, source_text, source_type);
    let scope_info = build_scope_info(module);
    let result = react_compiler::entrypoint::program::compile_program(file, scope_info, options);

    let diagnostics = compile_result_to_diagnostics(&result);
    let (program_json, events) = match result {
        react_compiler::entrypoint::compile_result::CompileResult::Success {
            ast, events, ..
        } => (ast, events),
        react_compiler::entrypoint::compile_result::CompileResult::Error { events, .. } => {
            (None, events)
        }
    };

    let conversion_result = program_json.and_then(|raw_json| {
        // First parse to serde_json::Value which deduplicates "type" fields
        // (the compiler output can produce duplicate "type" keys due to
        // BaseNode.node_type + #[serde(tag = "type")] enum tagging)
        let value: serde_json::Value = serde_json::from_str(raw_json.get()).ok()?;
        let file: react_compiler_ast::File = serde_json::from_value(value).ok()?;
        let result = convert_program_to_swc_with_source(&file, Some(source_text));
        Some(result)
    });

    let (mut swc_module, mut comments) = match conversion_result {
        Some(result) => (Some(result.module), Some(result.comments)),
        None => (None, None),
    };

    TransformResult {
        module: swc_module,
        comments,
        diagnostics,
        events,
    }
}
/// Lint API — same as transform but only collects diagnostics, no AST output.
pub fn lint(
    module: &swc_core::ecma::ast::Module,
    source_text: &str,
    options: PluginOptions,
) -> LintResult {
    let mut opts = options;
    opts.no_emit = true;

    let result = transform(module, source_text, opts);
    LintResult {
        diagnostics: result.diagnostics,
    }
}
