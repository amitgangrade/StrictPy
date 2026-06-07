//! M60: user-module imports.
//!
//! StrictPy v0.2 could only `import` built-in (compiler-internal) stdlib
//! modules — `from foo import bar` against a sibling `.spy` file errored
//! with `E4001 (user-defined modules are v0.3)`. This module lifts that
//! restriction.
//!
//! ## Strategy: compile-time module merging
//!
//! Rather than emit one `.spyc` per module and relocate every function id /
//! type id / constant index at *load* time (which would also force the JIT
//! to grow a cross-module call protocol), M60 resolves the whole import
//! graph **at compile time** and produces a single, self-contained merged
//! [`ast::Module`]. The rest of the pipeline (resolver → typecheck → IR →
//! bytecode → loader → interpreter → JIT) is unchanged: a cross-module call
//! is, after merging, an ordinary intra-module call, so it is automatically
//! correct in both the interpreter and the JIT.
//!
//! To keep names from different files from colliding, every top-level
//! definition of an imported module is **mangled** with a per-module prefix
//! derived from its dotted import path (`pkg.mod` → `__m60_pkg_mod__`). All
//! references *inside* that module to its own top-level names are rewritten
//! to the mangled form, respecting local shadowing (params, `let`s, `for`
//! vars, lambda params, `match`/`with`/`except` bindings, generics). In the
//! importing module:
//!
//!   * `import foo`            binds `foo` as a module alias; `foo.bar`,
//!                             `foo.Class(...)` and the type `foo.Class`
//!                             are rewritten to the mangled top-level name.
//!   * `from foo import a, B`  rewrites the bare names `a` / `B` (and the
//!                             type `B`) to their mangled forms.
//!
//! ## Resolution rules
//!
//! Module paths resolve **relative to the importing file's directory**:
//!
//!   * `import foo`        → `<dir>/foo.spy`
//!   * `import pkg.mod`    → `<dir>/pkg/mod.spy`   (a directory is a
//!                          package if it contains `.spy` files; no
//!                          `__init__` is required)
//!
//! If no such file exists, the import is left untouched and falls through to
//! the existing stdlib-import resolution in the resolver (so `import math`
//! etc. keep working).
//!
//! ## Circular imports
//!
//! The loader tracks an in-progress stack. If a module is encountered while
//! it is already being loaded, a clear `E4003` ImportError is raised rather
//! than infinitely recursing.
//!
//! ## Exports
//!
//! v1 rule: every top-level definition is importable. There is no `__all__`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{
    self, Arg, Block, ClassDecl, ConstDecl, Decorator, Expr, ExceptHandler, FieldDecl, FuncDecl,
    GenericParam, Import, Lvalue, MatchArm, Module, Pattern, ProtocolDecl, Stmt, TopDecl,
    Type, TypeAliasDecl,
};
use crate::error::{codes, CompileError};
use crate::{lexer, parser};

/// Parse `source` (whose on-disk location is `path`, used as the root for
/// relative import resolution) into a fully-merged [`Module`] with every
/// user-imported sibling module inlined and namespaced.
///
/// `path` may be a real filesystem path or a synthetic name; when it has no
/// existing parent directory, user-module resolution is simply a no-op and
/// the returned module is the parsed root unchanged (apart from leaving its
/// imports in place for the stdlib resolver).
pub fn resolve_and_merge(path: &str, source: &str) -> Result<Module, CompileError> {
    let root = parse_str(path, source)?;

    // Base directory for relative resolution: the directory containing the
    // root file. For synthetic paths (e.g. inline `-c` code or a bare
    // `foo.spy` test name with no directory) this is `.`; resolution then
    // only succeeds for files that actually exist there, which inline code
    // and self-contained snippets never reference.
    let base_dir = Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut loader = Loader {
        loaded: HashMap::new(),
        in_progress: Vec::new(),
        merged_decls: Vec::new(),
        stdlib_imports: Vec::new(),
        next_id: 0,
    };

    let merged_root = loader.process_module(root, &base_dir)?;

    // Assemble: root decls first (so `main` keeps its natural position),
    // then every inlined module's mangled decls.
    let mut decls = merged_root.decls;
    decls.append(&mut loader.merged_decls);

    Ok(Module {
        name: merged_root.name,
        imports: loader.stdlib_imports,
        decls,
        span: merged_root.span,
    })
}

/// Collect the transitive set of sibling user-module `.spy` files that
/// `path` (with the given `source`) imports, for cache-staleness checks in
/// the CLI. Best-effort: parse errors and cycles short-circuit gracefully
/// (the subsequent real compile will surface any diagnostic). The root file
/// itself is *not* included.
pub fn collect_import_deps(path: &str, source: &str) -> Vec<PathBuf> {
    let base_dir = Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut deps: Vec<PathBuf> = Vec::new();
    collect_deps_rec(&base_dir, source, &mut seen, &mut deps);
    deps
}

fn collect_deps_rec(
    base_dir: &Path,
    source: &str,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) {
    let module = match parse_str("<dep-scan>", source) {
        Ok(m) => m,
        Err(_) => return,
    };
    for imp in &module.imports {
        let dotted = imp.path.join(".");
        let mut p = base_dir.to_path_buf();
        for seg in dotted.split('.') {
            p.push(seg);
        }
        p.set_extension("spy");
        if p.is_file() && seen.insert(p.clone()) {
            // Recurse using the dependency's own directory as the new base.
            if let Ok(dep_src) = std::fs::read_to_string(&p) {
                let dep_base = p.parent().map(Path::to_path_buf).unwrap_or_else(|| base_dir.to_path_buf());
                out.push(p.clone());
                collect_deps_rec(&dep_base, &dep_src, seen, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn parse_str(path: &str, source: &str) -> Result<Module, CompileError> {
    let mut lx = lexer::Lexer::new(source);
    let mut tokens = Vec::new();
    loop {
        let tok = lx.next_token()?;
        let is_eof = matches!(tok.kind, lexer::TokenKind::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    let mut p = parser::Parser::new(tokens);
    let mut module = p.parse_module()?;
    // Stamp a human module name from the file stem for diagnostics.
    module.name = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&module.name)
        .to_string();
    Ok(module)
}

struct Loader {
    /// canonical module file → mangled prefix, for modules already merged.
    loaded: HashMap<PathBuf, String>,
    /// canonical module files currently being processed (cycle detection),
    /// paired with the dotted name used to reach them (for diagnostics).
    in_progress: Vec<(PathBuf, String)>,
    /// Mangled top-level decls accumulated from every inlined module.
    merged_decls: Vec<TopDecl>,
    /// Stdlib (non-user) imports preserved for the resolver to handle.
    stdlib_imports: Vec<Import>,
    /// Monotonic counter making every distinct module's prefix unique even
    /// if two files in different directories share a stem.
    next_id: u32,
}

impl Loader {
    /// Canonicalize a path for use as a stable module identity key. Falls
    /// back to the raw path if the OS canonicalization fails.
    fn canon(file: &Path) -> PathBuf {
        std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf())
    }

    /// Resolve a dotted import path to a sibling `.spy` file relative to
    /// `base_dir`. Returns `None` if no such file exists (→ stdlib/fallthrough).
    fn resolve_file(base_dir: &Path, dotted: &str) -> Option<PathBuf> {
        let mut p = base_dir.to_path_buf();
        for seg in dotted.split('.') {
            p.push(seg);
        }
        p.set_extension("spy");
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    }

    /// Process one module's imports. For the root, references are rewritten
    /// in place (import aliases + from-imports). For a non-root module, the
    /// caller has already self-mangled it; here we only recurse into *its*
    /// imports and accumulate.
    ///
    /// Returns the (possibly rewritten) module so the root's decls can be
    /// taken by the caller. `base_dir` is the directory of *this* module's
    /// source file — imports inside it resolve relative to it.
    fn process_module(&mut self, module: Module, base_dir: &Path) -> Result<Module, CompileError> {
        // Partition imports: user (resolvable to a sibling file) vs other
        // (stdlib / unknown — left for the resolver).
        //
        // For each user import we (1) ensure the target module is loaded &
        // merged (recursively), recording its prefix, and (2) build a rename
        // map / alias map for rewriting *this* module's body.
        let mut rename: HashMap<String, String> = HashMap::new();
        let mut module_aliases: HashMap<String, String> = HashMap::new();
        let mut remaining_imports: Vec<Import> = Vec::new();

        for imp in &module.imports {
            let dotted = imp.path.join(".");
            let file = Self::resolve_file(base_dir, &dotted);
            match file {
                None => {
                    // Not a user module — keep for stdlib resolution.
                    remaining_imports.push(imp.clone());
                    continue;
                }
                Some(file) => {
                    let prefix = self.ensure_loaded(&dotted, &file, imp.span)?;
                    if imp.items.is_empty() {
                        // `import foo` / `import pkg.mod [as alias]`
                        // The accessible name is the `as` alias if present,
                        // else the *full dotted path* (`pkg.geo`), matching
                        // how `pkg.geo.area(...)` is written at the use site.
                        let local = imp.alias.clone().unwrap_or_else(|| dotted.clone());
                        module_aliases.insert(local, prefix);
                    } else {
                        // `from foo import a, b as c`
                        for it in &imp.items {
                            let local = it.alias.clone().unwrap_or_else(|| it.name.clone());
                            rename.insert(local, format!("{prefix}{}", it.name));
                        }
                    }
                }
            }
        }

        // Stdlib imports only need to be collected once, at the root, because
        // a merged program is a single compilation unit and the resolver
        // dedups by name. (Imported user modules may themselves use stdlib;
        // those imports are hoisted to the root import list here.)
        for imp in remaining_imports {
            if !self
                .stdlib_imports
                .iter()
                .any(|e| e.path == imp.path && e.items.len() == imp.items.len() && e.alias == imp.alias)
            {
                self.stdlib_imports.push(imp);
            }
        }

        let mut module = module;
        module.imports = Vec::new();
        if !rename.is_empty() || !module_aliases.is_empty() {
            let mut r = Renamer::new(rename, module_aliases);
            r.rewrite_module(&mut module);
        }
        Ok(module)
    }

    /// Ensure the module at `dotted`/`file` is parsed, self-mangled, its own
    /// imports processed, and its decls accumulated into `merged_decls`.
    /// Returns the mangled prefix. Idempotent (keyed by canonical file path,
    /// so the same module reached under two different dotted names merges
    /// exactly once and shares one prefix → one set of types).
    fn ensure_loaded(
        &mut self,
        dotted: &str,
        file: &Path,
        span: ast::Span,
    ) -> Result<String, CompileError> {
        let key = Self::canon(file);

        if let Some(prefix) = self.loaded.get(&key) {
            return Ok(prefix.clone());
        }
        if self.in_progress.iter().any(|(p, _)| *p == key) {
            // Circular import: build a readable cycle trace of dotted names.
            let mut chain: Vec<String> =
                self.in_progress.iter().map(|(_, d)| d.clone()).collect();
            chain.push(dotted.to_string());
            return Err(CompileError::Semantic {
                file: file.display().to_string(),
                line: span.line,
                col: span.col,
                code: codes::LINK_CIRCULAR_IMPORT,
                message: format!("circular import detected: {}", chain.join(" -> ")),
            });
        }

        let source = std::fs::read_to_string(file).map_err(CompileError::Io)?;
        let parsed = parse_str(&file.display().to_string(), &source)?;

        // Unique prefix per distinct file (id-suffixed so two files with the
        // same stem in different dirs never collide).
        let id = self.next_id;
        self.next_id += 1;
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mod");
        let san: String = stem
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let prefix = format!("__m60_{san}_{id}__");

        self.in_progress.push((key.clone(), dotted.to_string()));

        // Self-mangle: rename this module's own top-level names to the
        // prefixed form everywhere in its body, respecting shadowing.
        let mut module = parsed;
        let own_names = top_level_names(&module);
        let self_rename: HashMap<String, String> = own_names
            .into_iter()
            .map(|n| (n.clone(), format!("{prefix}{n}")))
            .collect();
        {
            let mut r = Renamer::new(self_rename.clone(), HashMap::new());
            r.rewrite_module(&mut module);
        }
        // Rename the *declarations* themselves (the renamer above only
        // rewrites references, not the defining occurrences).
        rename_decl_names(&mut module, &self_rename);

        // This module's own imports resolve relative to *its* directory.
        let dep_base = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        // Process this module's own imports (recursively merges deps and
        // rewrites cross-module references inside this module). Note we mark
        // it loaded BEFORE recursing is NOT done — cycle detection relies on
        // the in_progress stack; `loaded` is set only after full success.
        let processed = self.process_module(module, &dep_base)?;

        // Accumulate its (now mangled + import-rewritten) decls.
        for d in processed.decls {
            self.merged_decls.push(d);
        }

        self.in_progress.pop();
        self.loaded.insert(key, prefix.clone());
        Ok(prefix)
    }
}

/// Collect the set of top-level definition names in a module.
fn top_level_names(module: &Module) -> HashSet<String> {
    let mut s = HashSet::new();
    for d in &module.decls {
        match d {
            TopDecl::Func(f) => {
                s.insert(f.name.clone());
            }
            TopDecl::Class(c) => {
                s.insert(c.name.clone());
            }
            TopDecl::Protocol(p) => {
                s.insert(p.name.clone());
            }
            TopDecl::Const(c) => {
                s.insert(c.name.clone());
            }
            TopDecl::TypeAlias(t) => {
                s.insert(t.name.clone());
            }
        }
    }
    s
}

/// Rename the *defining* occurrences of top-level decls per `map`.
fn rename_decl_names(module: &mut Module, map: &HashMap<String, String>) {
    for d in &mut module.decls {
        match d {
            TopDecl::Func(f) => remap(&mut f.name, map),
            TopDecl::Class(c) => remap(&mut c.name, map),
            TopDecl::Protocol(p) => remap(&mut p.name, map),
            TopDecl::Const(c) => remap(&mut c.name, map),
            TopDecl::TypeAlias(t) => remap(&mut t.name, map),
        }
    }
}

fn remap(name: &mut String, map: &HashMap<String, String>) {
    if let Some(n) = map.get(name) {
        *name = n.clone();
    }
}

// ───────────────────────────────────────────────────────────────────────────
//  Scoped reference renamer
// ───────────────────────────────────────────────────────────────────────────

/// Rewrites *references* (not binding sites) to names in `rename`, and
/// rewrites module-qualified access `alias.member` to the mangled member
/// name using `module_aliases` (alias → mangle-prefix).
///
/// A `shadow` stack tracks locally-bound names so that a `let foo = ...`
/// (or a param / for-var / lambda param / pattern binding / generic) that
/// happens to collide with an imported name is *not* rewritten within its
/// scope.
struct Renamer {
    rename: HashMap<String, String>,
    module_aliases: HashMap<String, String>,
    /// Stack of shadowing scopes; each frame is the set of names bound there.
    shadow: Vec<HashSet<String>>,
}

impl Renamer {
    fn new(rename: HashMap<String, String>, module_aliases: HashMap<String, String>) -> Self {
        Renamer {
            rename,
            module_aliases,
            shadow: vec![HashSet::new()],
        }
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.shadow.iter().any(|f| f.contains(name))
    }

    fn bind(&mut self, name: &str) {
        if let Some(f) = self.shadow.last_mut() {
            f.insert(name.to_string());
        }
    }

    fn push_scope(&mut self) {
        self.shadow.push(HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.shadow.pop();
    }

    /// Rewrite a bare identifier *reference*.
    fn rewrite_ident(&self, name: &mut String) {
        if self.is_shadowed(name) {
            return;
        }
        if let Some(n) = self.rename.get(name) {
            *name = n.clone();
        }
    }

    fn rewrite_module(&mut self, module: &mut Module) {
        for d in &mut module.decls {
            self.rewrite_topdecl(d);
        }
    }

    fn rewrite_topdecl(&mut self, d: &mut TopDecl) {
        match d {
            TopDecl::Func(f) => self.rewrite_func(f),
            TopDecl::Class(c) => self.rewrite_class(c),
            TopDecl::Protocol(p) => self.rewrite_protocol(p),
            TopDecl::Const(c) => self.rewrite_const(c),
            TopDecl::TypeAlias(t) => self.rewrite_type_alias(t),
        }
    }

    fn rewrite_func(&mut self, f: &mut FuncDecl) {
        self.push_scope();
        // Generic params shadow.
        for g in &f.generics {
            self.bind(&g.name);
        }
        for g in &mut f.generics {
            self.rewrite_generic(g);
        }
        for p in &mut f.params {
            self.rewrite_type(&mut p.ty);
            if let Some(d) = &mut p.default {
                self.rewrite_expr(d);
            }
        }
        // Params bind after their annotations/defaults are rewritten.
        for p in &f.params.clone() {
            self.bind(&p.name);
        }
        self.rewrite_type(&mut f.return_ty);
        for dec in &mut f.decorators {
            self.rewrite_decorator(dec);
        }
        self.rewrite_block(&mut f.body);
        self.pop_scope();
    }

    fn rewrite_generic(&mut self, g: &mut GenericParam) {
        for b in &mut g.bounds {
            self.rewrite_type(b);
        }
    }

    fn rewrite_class(&mut self, c: &mut ClassDecl) {
        self.push_scope();
        for g in &c.generics {
            self.bind(&g.name);
        }
        for g in &mut c.generics {
            self.rewrite_generic(g);
        }
        for b in &mut c.bases {
            self.rewrite_type(b);
        }
        for fld in &mut c.fields {
            self.rewrite_field(fld);
        }
        if let Some(init) = &mut c.init {
            self.rewrite_func(init);
        }
        for m in &mut c.methods {
            self.rewrite_func(m);
        }
        self.pop_scope();
    }

    fn rewrite_field(&mut self, fld: &mut FieldDecl) {
        self.rewrite_type(&mut fld.ty);
        if let Some(d) = &mut fld.default {
            self.rewrite_expr(d);
        }
    }

    fn rewrite_protocol(&mut self, p: &mut ProtocolDecl) {
        self.push_scope();
        for g in &p.generics {
            self.bind(&g.name);
        }
        for g in &mut p.generics {
            self.rewrite_generic(g);
        }
        for m in &mut p.methods {
            for prm in &mut m.params {
                self.rewrite_type(&mut prm.ty);
            }
            self.rewrite_type(&mut m.return_ty);
        }
        self.pop_scope();
    }

    fn rewrite_const(&mut self, c: &mut ConstDecl) {
        self.rewrite_type(&mut c.ty);
        self.rewrite_expr(&mut c.value);
    }

    fn rewrite_type_alias(&mut self, t: &mut TypeAliasDecl) {
        self.push_scope();
        for g in &t.generics {
            self.bind(&g.name);
        }
        self.rewrite_type(&mut t.ty);
        self.pop_scope();
    }

    fn rewrite_decorator(&mut self, dec: &mut Decorator) {
        if let Some(first) = dec.path.first_mut() {
            // A decorator referencing an imported callable.
            if !self.is_shadowed(first) {
                if let Some(n) = self.rename.get(first.as_str()) {
                    *first = n.clone();
                }
            }
        }
        for a in &mut dec.args {
            self.rewrite_arg(a);
        }
    }

    // ── Types ───────────────────────────────────────────────────────────

    fn rewrite_type(&mut self, ty: &mut Type) {
        match ty {
            Type::Named { name, args, .. } => {
                // Module-qualified type `alias.Member` → mangled member.
                if name.contains('.') {
                    // Module-qualified type `alias.Member` or
                    // `pkg.sub.Member`. Match the *longest* registered alias
                    // that is a dotted prefix of `name`, then mangle the
                    // remaining member segment(s).
                    let head = name.split('.').next().unwrap_or(name.as_str());
                    if !self.is_shadowed(head) {
                        let mut best: Option<(usize, String)> = None;
                        for (alias, prefix) in &self.module_aliases {
                            let with_dot = format!("{alias}.");
                            if name.starts_with(&with_dot) && alias.len() > best.as_ref().map_or(0, |b| b.0) {
                                best = Some((alias.len(), prefix.clone()));
                            }
                        }
                        if let Some((alias_len, prefix)) = best {
                            let member = &name[alias_len + 1..]; // skip "alias."
                            *name = format!("{prefix}{member}");
                        }
                    }
                } else if !self.is_shadowed(name) {
                    if let Some(n) = self.rename.get(name) {
                        *name = n.clone();
                    }
                }
                for a in args {
                    self.rewrite_type(a);
                }
            }
            Type::Nullable { inner, .. } => self.rewrite_type(inner),
            Type::Function { params, ret, .. } => {
                for p in params {
                    self.rewrite_type(p);
                }
                self.rewrite_type(ret);
            }
            Type::Tuple { elems, .. } => {
                for e in elems {
                    self.rewrite_type(e);
                }
            }
            Type::Infer { .. } | Type::Never { .. } => {}
        }
    }

    // ── Statements / blocks ─────────────────────────────────────────────

    fn rewrite_block(&mut self, block: &mut Block) {
        self.push_scope();
        for s in &mut block.stmts {
            self.rewrite_stmt(s);
        }
        self.pop_scope();
    }

    /// Rewrite a block *without* opening a fresh shadow scope — used when the
    /// caller has already opened one to host loop/with bindings.
    fn rewrite_block_no_scope(&mut self, block: &mut Block) {
        for s in &mut block.stmts {
            self.rewrite_stmt(s);
        }
    }

    fn rewrite_stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let { name, ty, init, .. } => {
                self.rewrite_type(ty);
                self.rewrite_expr(init);
                // The new binding shadows from here on in this scope.
                self.bind(name);
            }
            Stmt::LetDestructure { names, tys, init, .. } => {
                for t in tys.iter_mut().flatten() {
                    self.rewrite_type(t);
                }
                self.rewrite_expr(init);
                for n in names.iter() {
                    self.bind(n);
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.rewrite_expr(value);
                self.rewrite_lvalue(target);
            }
            Stmt::AugAssign { target, value, .. } => {
                self.rewrite_expr(value);
                self.rewrite_lvalue(target);
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.rewrite_expr(v);
                }
            }
            Stmt::If { cond, then_block, elifs, else_block, .. } => {
                self.rewrite_expr(cond);
                self.rewrite_block(then_block);
                for (c, b) in elifs {
                    self.rewrite_expr(c);
                    self.rewrite_block(b);
                }
                if let Some(b) = else_block {
                    self.rewrite_block(b);
                }
            }
            Stmt::While { cond, body, else_block, .. } => {
                self.rewrite_expr(cond);
                self.rewrite_block(body);
                if let Some(b) = else_block {
                    self.rewrite_block(b);
                }
            }
            Stmt::For { var, var_ty, iter, body, else_block, .. } => {
                self.rewrite_expr(iter);
                self.rewrite_type(var_ty);
                self.push_scope();
                self.bind(var);
                self.rewrite_block_no_scope(body);
                self.pop_scope();
                if let Some(b) = else_block {
                    self.rewrite_block(b);
                }
            }
            Stmt::Match { scrutinee, arms, .. } => {
                self.rewrite_expr(scrutinee);
                for arm in arms {
                    self.rewrite_match_arm(arm);
                }
            }
            Stmt::Try { body, handlers, else_block, finally_block, .. } => {
                self.rewrite_block(body);
                for h in handlers {
                    self.rewrite_except_handler(h);
                }
                if let Some(b) = else_block {
                    self.rewrite_block(b);
                }
                if let Some(b) = finally_block {
                    self.rewrite_block(b);
                }
            }
            Stmt::With { expr, binding, body, .. } => {
                self.rewrite_expr(expr);
                self.push_scope();
                if let Some((name, ty)) = binding {
                    self.rewrite_type(ty);
                    self.bind(name);
                }
                self.rewrite_block_no_scope(body);
                self.pop_scope();
            }
            Stmt::Raise { exc, cause, .. } => {
                self.rewrite_expr(exc);
                if let Some(c) = cause {
                    self.rewrite_expr(c);
                }
            }
            Stmt::Assert { cond, msg, .. } => {
                self.rewrite_expr(cond);
                if let Some(m) = msg {
                    self.rewrite_expr(m);
                }
            }
            Stmt::Del { target, .. } => self.rewrite_lvalue(target),
            Stmt::Expr { expr, .. } => self.rewrite_expr(expr),
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Pass { .. } => {}
        }
    }

    fn rewrite_match_arm(&mut self, arm: &mut MatchArm) {
        self.push_scope();
        self.rewrite_pattern(&mut arm.pattern);
        if let Some(g) = &mut arm.guard {
            self.rewrite_expr(g);
        }
        self.rewrite_block_no_scope(&mut arm.body);
        self.pop_scope();
    }

    fn rewrite_pattern(&mut self, p: &mut Pattern) {
        match p {
            Pattern::Literal(_, _) => {}
            // An identifier pattern *binds* a fresh name (capture); it is not
            // a reference, so we record the shadow and do not rewrite it.
            Pattern::Identifier(name, _) => self.bind(name),
            Pattern::Wildcard(_) => {}
            Pattern::Constructor { ty, fields, .. } => {
                self.rewrite_type(ty);
                for f in fields {
                    self.rewrite_pattern(f);
                }
            }
            Pattern::Tuple(elems, _) => {
                for e in elems {
                    self.rewrite_pattern(e);
                }
            }
        }
    }

    fn rewrite_except_handler(&mut self, h: &mut ExceptHandler) {
        self.rewrite_type(&mut h.exc_ty);
        self.push_scope();
        if let Some(b) = &h.binding {
            self.bind(b);
        }
        self.rewrite_block_no_scope(&mut h.body);
        self.pop_scope();
    }

    // ── Lvalues / expressions ───────────────────────────────────────────

    fn rewrite_lvalue(&mut self, lv: &mut Lvalue) {
        match lv {
            Lvalue::Ident { name, .. } => self.rewrite_ident(name),
            Lvalue::Attr { obj, .. } => self.rewrite_expr(obj),
            Lvalue::Index { obj, indices, .. } => {
                self.rewrite_expr(obj);
                for i in indices {
                    self.rewrite_expr(i);
                }
            }
        }
    }

    fn rewrite_arg(&mut self, a: &mut Arg) {
        self.rewrite_expr(&mut a.value);
    }

    fn rewrite_expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Literal { .. } => {}
            Expr::Ident { name, .. } => self.rewrite_ident(name),
            Expr::Tuple { elems, .. } | Expr::List { elems, .. } | Expr::Set { elems, .. } => {
                for el in elems {
                    self.rewrite_expr(el);
                }
            }
            Expr::Dict { entries, .. } => {
                for (k, v) in entries {
                    self.rewrite_expr(k);
                    self.rewrite_expr(v);
                }
            }
            Expr::Unary { operand, .. } => self.rewrite_expr(operand),
            Expr::Binary { lhs, rhs, .. } => {
                self.rewrite_expr(lhs);
                self.rewrite_expr(rhs);
            }
            Expr::Call { callee, args, .. } => {
                // Special case: a module-qualified call `alias.member(...)`
                // that happened to parse as Call(Attr, ..) rather than a
                // MethodCall (defensive — the parser folds `ident.m(..)` into
                // a MethodCall, but a parenthesised receiver would not).
                if let Some(rewritten) = self.try_module_attr_to_ident(&**callee) {
                    **callee = rewritten;
                } else {
                    self.rewrite_expr(callee);
                }
                for a in args {
                    self.rewrite_arg(a);
                }
            }
            Expr::MethodCall { receiver, method, args, span } => {
                // `alias.member(args)` parses as a MethodCall when `alias` is
                // a bare ident (the parser folds `ident.m(..)`). If `alias`
                // is a module alias (`foo` or dotted `pkg.geo`), this is
                // really a free-function call into that module: rewrite to
                // Call(Ident(prefix+member), args).
                let alias_prefix = self.module_alias_of(&**receiver);
                if let Some(prefix) = alias_prefix {
                    let new_name = format!("{prefix}{method}");
                    let sp = *span;
                    let mut new_args = std::mem::take(args);
                    for a in &mut new_args {
                        self.rewrite_arg(a);
                    }
                    *e = Expr::Call {
                        callee: Box::new(Expr::Ident { name: new_name, span: sp }),
                        args: new_args,
                        span: sp,
                    };
                } else {
                    self.rewrite_expr(receiver);
                    for a in args {
                        self.rewrite_arg(a);
                    }
                }
            }
            Expr::Attr { obj, name, span } => {
                // `alias.member` value access → mangled ident.
                let alias_prefix = self.module_alias_of(&**obj);
                if let Some(prefix) = alias_prefix {
                    let new_name = format!("{prefix}{name}");
                    let sp = *span;
                    *e = Expr::Ident { name: new_name, span: sp };
                } else {
                    self.rewrite_expr(obj);
                }
            }
            Expr::Index { obj, indices, .. } => {
                self.rewrite_expr(obj);
                for i in indices {
                    self.rewrite_expr(i);
                }
            }
            Expr::NullCoalesce { lhs, rhs, .. } => {
                self.rewrite_expr(lhs);
                self.rewrite_expr(rhs);
            }
            Expr::Ternary { cond, then_expr, else_expr, .. } => {
                self.rewrite_expr(cond);
                self.rewrite_expr(then_expr);
                self.rewrite_expr(else_expr);
            }
            Expr::Lambda { params, return_ty, body, .. } => {
                self.push_scope();
                for p in params.iter_mut() {
                    self.rewrite_type(&mut p.ty);
                }
                for p in params.iter() {
                    self.bind(&p.name);
                }
                self.rewrite_type(return_ty);
                self.rewrite_expr(body);
                self.pop_scope();
            }
            Expr::Cast { expr, target, .. } => {
                self.rewrite_expr(expr);
                self.rewrite_type(target);
            }
            Expr::Comprehension { var, var_ty, iter, body, value, cond, .. } => {
                // Rewrite the iterable in the outer scope, then bind the loop
                // variable for the body / value / filter.
                self.rewrite_expr(iter);
                self.push_scope();
                self.rewrite_type(var_ty);
                self.bind(var);
                self.rewrite_expr(body);
                if let Some(v) = value { self.rewrite_expr(v); }
                if let Some(c) = cond { self.rewrite_expr(c); }
                self.pop_scope();
            }
        }
    }

    /// Flatten a chain of `Ident`/`Attr` (`pkg`, `pkg.geo`, ...) into its
    /// dotted string form. Returns `None` if the chain contains anything
    /// other than plain identifiers/attrs, or if its head ident is locally
    /// shadowed (in which case it is a value, not a module reference).
    fn flatten_dotted(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident { name, .. } => {
                if self.is_shadowed(name) {
                    None
                } else {
                    Some(name.clone())
                }
            }
            Expr::Attr { obj, name, .. } => {
                let head = self.flatten_dotted(obj)?;
                Some(format!("{head}.{name}"))
            }
            _ => None,
        }
    }

    /// If `obj` flattens to a registered module alias (`foo` or `pkg.geo`),
    /// return that module's mangle prefix.
    fn module_alias_of(&self, obj: &Expr) -> Option<String> {
        let dotted = self.flatten_dotted(obj)?;
        // The head segment must not be a locally-shadowed name.
        let head = dotted.split('.').next().unwrap_or(&dotted);
        if self.is_shadowed(head) {
            return None;
        }
        self.module_aliases.get(&dotted).cloned()
    }

    /// Convert a callee expression `alias.member` into a plain `Ident`
    /// expression `prefix+member` when `alias` is a module alias. Returns the
    /// replacement expr, or `None` if not applicable.
    fn try_module_attr_to_ident(&self, callee: &Expr) -> Option<Expr> {
        if let Expr::Attr { obj, name, span } = callee {
            if let Some(prefix) = self.module_alias_of(obj) {
                return Some(Expr::Ident {
                    name: format!("{prefix}{name}"),
                    span: *span,
                });
            }
        }
        None
    }
}
