use crate::ast::{Item, Program, Type, UseStatement, UseTree};
use crate::lexer;
use crate::parser;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModuleError {
    #[error("Module not found: {0}")]
    ModuleNotFound(String),
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),
    #[error("Failed to read module: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse module: {0}")]
    ParseError(String),
}

pub struct ModuleLoader {
    loaded_modules: HashSet<PathBuf>,
    loading_stack: Vec<PathBuf>,
    base_path: PathBuf,
}

impl ModuleLoader {
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            loaded_modules: HashSet::new(),
            loading_stack: Vec::new(),
            base_path,
        }
    }

    pub fn load_program(&mut self, main_file: &Path) -> Result<Program, ModuleError> {
        let source = fs::read_to_string(main_file)?;
        let tokens = lexer::tokenize(&source)
            .map_err(|e| ModuleError::ParseError(format!("Lexer error: {}", e)))?;
        let mut program = parser::parse(tokens)
            .map_err(|e| ModuleError::ParseError(format!("Parser error: {}", e)))?;

        // Add to loading stack
        self.loading_stack.push(main_file.to_path_buf());

        // Process imports
        let mut all_items = Vec::new();
        for use_stmt in &program.imports {
            let imported_items = self.load_module_from_use(use_stmt, main_file)?;
            all_items.extend(imported_items);
        }

        // Remove from loading stack
        self.loading_stack.pop();

        // Mark this module as loaded
        self.loaded_modules.insert(main_file.to_path_buf());

        // Add the main module's items
        all_items.extend(program.items);
        program.items = all_items;

        Ok(program)
    }

    fn load_module_from_use(
        &mut self,
        use_stmt: &UseStatement,
        current_file: &Path,
    ) -> Result<Vec<Item>, ModuleError> {
        // Convert the use path to a file path
        let module_file_path = self.resolve_use_path(&use_stmt.path, current_file)?;

        // Check for circular dependencies (only in current loading stack)
        if self.loading_stack.contains(&module_file_path) {
            return Err(ModuleError::CircularDependency(
                module_file_path.display().to_string(),
            ));
        }

        // If already loaded, skip to avoid duplication
        if self.loaded_modules.contains(&module_file_path) {
            return Ok(Vec::new());
        }

        // Load and parse the module
        let source = fs::read_to_string(&module_file_path)?;
        let tokens = lexer::tokenize(&source).map_err(|e| {
            ModuleError::ParseError(format!(
                "Lexer error in {}: {}",
                module_file_path.display(),
                e
            ))
        })?;
        let module_program = parser::parse(tokens).map_err(|e| {
            ModuleError::ParseError(format!(
                "Parser error in {}: {}",
                module_file_path.display(),
                e
            ))
        })?;

        // Add to loading stack before processing nested imports
        self.loading_stack.push(module_file_path.clone());

        // Process nested imports in the module
        let mut module_items = Vec::new();
        for nested_use in &module_program.imports {
            let imported_items = self.load_module_from_use(nested_use, &module_file_path)?;
            module_items.extend(imported_items);
        }

        // Remove from loading stack
        self.loading_stack.pop();

        // Mark this module as loaded
        self.loaded_modules.insert(module_file_path.clone());

        // Filter items based on use tree specification
        let filtered_items = self.filter_items_by_use_tree(&module_program.items, &use_stmt.items);

        module_items.extend(filtered_items);
        Ok(module_items)
    }

    fn filter_items_by_use_tree(&self, items: &[Item], tree: &UseTree) -> Vec<Item> {
        match tree {
            UseTree::Glob => items.to_vec(),
            UseTree::Simple(name) => {
                let mut result = Vec::new();
                // First add the named item itself
                for item in items {
                    if get_item_name(item) == Some(name.as_str()) {
                        result.push(item.clone());
                    }
                }
                // Then add all impl blocks that reference this item
                for item in items {
                    if let Item::Impl(impl_block) = item {
                        // Check if this impl is for the imported type or trait
                        if let Type::Path(type_name) = &impl_block.target_type {
                            if type_name == name {
                                result.push(item.clone());
                            }
                        }
                        // Also include if it's implementing the imported trait
                        if let Some(trait_name) = &impl_block.trait_name {
                            if trait_name == name {
                                result.push(item.clone());
                            }
                        }
                    }
                }
                result
            }
            UseTree::List(trees) => {
                let mut result = Vec::new();
                let mut imported_names = Vec::new();

                // First collect all imported names
                for tree in trees {
                    if let UseTree::Simple(name) | UseTree::Alias(name, _) = tree {
                        imported_names.push(name.clone());
                    }
                }

                // Then add items and their implementations
                for tree in trees {
                    result.extend(self.filter_items_by_use_tree(items, tree));
                }

                // Also add impl blocks that reference any of the imported items
                for item in items {
                    if let Item::Impl(impl_block) = item {
                        // Check if this impl is for any imported type
                        if let Type::Path(type_name) = &impl_block.target_type {
                            if imported_names.contains(type_name) && !result.contains(item) {
                                result.push(item.clone());
                            }
                        }
                        // Also include if it's implementing any imported trait
                        if let Some(trait_name) = &impl_block.trait_name {
                            if imported_names.contains(trait_name) && !result.contains(item) {
                                result.push(item.clone());
                            }
                        }
                    }
                }

                result
            }
            UseTree::Alias(name, _alias) => {
                // For now, aliases are handled in codegen, just import the original item
                let mut result = Vec::new();
                for item in items {
                    if get_item_name(item) == Some(name.as_str()) {
                        result.push(item.clone());
                    }
                }
                // Also add impl blocks for this type
                for item in items {
                    if let Item::Impl(impl_block) = item {
                        if let Type::Path(type_name) = &impl_block.target_type {
                            if type_name == name {
                                result.push(item.clone());
                            }
                        }
                        if let Some(trait_name) = &impl_block.trait_name {
                            if trait_name == name {
                                result.push(item.clone());
                            }
                        }
                    }
                }
                result
            }
        }
    }

    fn resolve_use_path(
        &self,
        path_segments: &[String],
        current_file: &Path,
    ) -> Result<PathBuf, ModuleError> {
        // Convert path segments to file path
        // e.g., ["crate", "module", "submodule"] -> "module/submodule"
        // e.g., ["super", "module"] -> "../module"
        // e.g., ["module"] -> "module"

        let file_path = if path_segments.is_empty() {
            return Err(ModuleError::ModuleNotFound("empty path".to_string()));
        } else if path_segments[0] == "crate" {
            // crate:: refers to the root of the current crate
            path_segments[1..].join("/")
        } else if path_segments[0] == "super" {
            // super:: refers to the parent module
            let mut path = String::from("..");
            for segment in &path_segments[1..] {
                path.push('/');
                path.push_str(segment);
            }
            path
        } else {
            // Regular module path
            path_segments.join("/")
        };

        // Try different extensions
        let extensions = ["", ".rico8", ".r8"];

        // Get the directory of the current file
        let current_dir = current_file.parent().unwrap_or(&self.base_path);

        for ext in &extensions {
            let path_with_ext = format!("{}{}", file_path, ext);

            // For crate:: paths, start from base path
            if !path_segments.is_empty() && path_segments[0] == "crate" {
                let crate_path = self.base_path.join(&path_with_ext);
                if crate_path.exists() {
                    return Ok(crate_path);
                }
            } else {
                // Try relative to current file
                let relative_path = current_dir.join(&path_with_ext);
                if relative_path.exists() {
                    return Ok(relative_path);
                }

                // Try relative to base path
                let base_path = self.base_path.join(&path_with_ext);
                if base_path.exists() {
                    return Ok(base_path);
                }
            }
        }

        Err(ModuleError::ModuleNotFound(file_path))
    }
}

fn get_item_name(item: &Item) -> Option<&str> {
    match item {
        Item::Struct(s) => Some(&s.name),
        Item::Enum(e) => Some(&e.name),
        Item::Trait(t) => Some(&t.name),
        Item::Function(f) => Some(&f.name),
        Item::Const(c) => Some(&c.name),
        Item::Impl(_) | Item::Global(_) => None,
    }
}
