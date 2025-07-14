//! Warning tracking and management for GHC diagnostics.
//!
//! This module provides functionality to track warnings across recompilations,
//! managing the lifecycle of warnings as files are modified, added, or removed.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};

use crate::ghci::parse::GhcDiagnostic;
use crate::ghci::CompilationLog;
use crate::normal_path::NormalPath;

/// Tracks warnings across recompilations, managing their lifecycle.
///
/// This tracker maintains an in-memory store of warnings per file and provides
/// smart logic for updating warnings based on compilation results and file changes.
pub struct WarningTracker {
    /// Per-file warnings from GHC compilation, persisted across reloads.
    warnings: BTreeMap<NormalPath, Vec<GhcDiagnostic>>,
    /// Files that were directly changed in the current reload operation.
    /// Used to distinguish between direct changes and dependency-driven recompilations.
    current_changed_files: BTreeSet<NormalPath>,
}

impl WarningTracker {
    /// Create a new warning tracker.
    pub fn new() -> Self {
        Self {
            warnings: BTreeMap::new(),
            current_changed_files: BTreeSet::new(),
        }
    }

    /// Reset the list of files that were directly changed.
    /// This should be called at the start of each reload operation.
    pub fn reset_changed_files(&mut self) {
        self.current_changed_files.clear();
    }

    /// Mark a file as having been directly changed.
    /// This affects how warnings are managed for this file.
    pub fn mark_file_changed(&mut self, path: NormalPath) {
        self.current_changed_files.insert(path);
    }

    /// Update warnings from a compilation log.
    ///
    /// This method implements smart warning persistence logic:
    /// - Files that were directly changed: always update warnings (or clear if none)
    /// - Files that were recompiled due to dependencies: only update if warnings exist
    /// - Files that weren't recompiled: keep existing warnings
    pub fn update_warnings_from_log(&mut self, log: &CompilationLog) {
        // Extract new warnings by file from the compilation log
        let mut new_warnings_by_file: BTreeMap<NormalPath, Vec<GhcDiagnostic>> = BTreeMap::new();

        for diagnostic in &log.diagnostics {
            if let Some(path) = &diagnostic.path {
                // Convert to NormalPath - in a real implementation, this would need proper error handling
                if let Ok(normal_path) = NormalPath::new(path, std::env::current_dir().unwrap()) {
                    new_warnings_by_file
                        .entry(normal_path)
                        .or_default()
                        .push(diagnostic.clone());
                }
            }
        }

        // Process compiled files from the log
        for compiled_module in &log.compiled_modules {
            // Convert module path to NormalPath
            if let Ok(compiled_file) =
                NormalPath::new(&compiled_module.path, std::env::current_dir().unwrap())
            {
                if let Some(file_warnings) = new_warnings_by_file.get(&compiled_file) {
                    // File was compiled and has warnings - always update them
                    self.warnings
                        .insert(compiled_file.clone(), file_warnings.clone());
                } else if self.current_changed_files.contains(&compiled_file) {
                    // File was directly changed and compiled but has no warnings - clear existing warnings
                    self.warnings.remove(&compiled_file);
                }
                // If file was compiled due to dependencies and has no warnings, keep existing warnings
            }
        }

        tracing::debug!(
            total_files_with_warnings = self.warnings.len(),
            total_warnings = self.warnings.values().map(|w| w.len()).sum::<usize>(),
            "Updated warnings from compilation log"
        );
    }

    /// Clear warnings for the specified paths.
    /// This is called when files are removed or when we know they should no longer have warnings.
    pub fn clear_warnings_for_paths<P>(&mut self, paths: impl IntoIterator<Item = P>)
    where
        P: Borrow<NormalPath>,
    {
        for path in paths {
            self.warnings.remove(path.borrow());
        }

        tracing::debug!(
            files_with_warnings = self.warnings.len(),
            total_warnings = self.warnings.values().map(|w| w.len()).sum::<usize>(),
            "Cleared warnings for paths"
        );
    }

    /// Get all warnings as a map from file path to warnings.
    pub fn get_all_warnings(&self) -> &BTreeMap<NormalPath, Vec<GhcDiagnostic>> {
        &self.warnings
    }

    /// Get the total number of warnings across all files.
    pub fn warning_count(&self) -> usize {
        self.warnings.values().map(|w| w.len()).sum()
    }

    /// Check if there are any warnings tracked.
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

impl Default for WarningTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghci::parse::CompilingModule;
    use crate::ghci::parse::PositionRange;
    use crate::ghci::parse::{GhcDiagnostic, Severity};

    fn create_test_diagnostic(severity: Severity, path: &str, message: &str) -> GhcDiagnostic {
        GhcDiagnostic {
            severity,
            path: Some(path.into()),
            span: PositionRange::new(1, 1, 1, 1),
            message: message.to_string(),
        }
    }

    fn create_test_compilation_log(
        diagnostics: Vec<GhcDiagnostic>,
        compiled_modules: Vec<CompilingModule>,
    ) -> CompilationLog {
        CompilationLog {
            diagnostics,
            compiled_modules,
            summary: None,
        }
    }

    #[test]
    fn test_new_tracker_is_empty() {
        let tracker = WarningTracker::new();
        assert!(!tracker.has_warnings());
        assert_eq!(tracker.warning_count(), 0);
        assert_eq!(tracker.warnings.len(), 0);
    }

    #[test]
    fn test_mark_file_changed() {
        let mut tracker = WarningTracker::new();
        let base_dir = std::env::current_dir().unwrap();
        let path = NormalPath::new("src/test.hs", &base_dir).unwrap();

        tracker.mark_file_changed(path.clone());
        assert!(tracker.current_changed_files.contains(&path));

        tracker.reset_changed_files();
        assert!(!tracker.current_changed_files.contains(&path));
    }

    #[test]
    fn test_clear_warnings_for_paths() {
        let mut tracker = WarningTracker::new();
        let base_dir = std::env::current_dir().unwrap();
        let path1 = NormalPath::new("src/test1.hs", &base_dir).unwrap();
        let path2 = NormalPath::new("src/test2.hs", &base_dir).unwrap();

        let warnings = vec![create_test_diagnostic(
            Severity::Warning,
            "src/test1.hs",
            "unused import",
        )];

        tracker.warnings.insert(path1.clone(), warnings.clone());
        tracker.warnings.insert(path2.clone(), warnings);
        assert_eq!(tracker.warnings.len(), 2);

        tracker.clear_warnings_for_paths([&path1]);
        assert_eq!(tracker.warnings.len(), 1);
        assert!(!tracker.warnings.contains_key(&path1));
        assert!(tracker.warnings.contains_key(&path2));
    }

    #[test]
    fn test_update_warnings_from_log_direct_change() {
        let mut tracker = WarningTracker::new();
        let base_dir = std::env::current_dir().unwrap();
        let path = NormalPath::new("src/test.hs", &base_dir).unwrap();

        // Add initial warnings
        let old_warning = create_test_diagnostic(Severity::Warning, "src/test.hs", "old warning");
        tracker.warnings.insert(path.clone(), vec![old_warning]);

        // Mark file as changed
        tracker.mark_file_changed(path.clone());

        // Create compilation log with no warnings for the changed file
        let log = create_test_compilation_log(
            vec![], // No warnings in this compilation
            vec![CompilingModule {
                name: "Test".to_string(),
                path: "src/test.hs".into(),
            }],
        );

        tracker.update_warnings_from_log(&log);

        // Warnings should be cleared since file was directly changed and has no new warnings
        assert!(!tracker.has_warnings());
    }

    #[test]
    fn test_update_warnings_from_log_dependency_change() {
        let mut tracker = WarningTracker::new();
        let base_dir = std::env::current_dir().unwrap();
        let path = NormalPath::new("src/test.hs", &base_dir).unwrap();

        // Add initial warnings
        let old_warning = create_test_diagnostic(Severity::Warning, "src/test.hs", "old warning");
        tracker
            .warnings
            .insert(path.clone(), vec![old_warning.clone()]);

        // Don't mark file as changed (dependency-driven recompilation)

        // Create compilation log with no warnings for the file
        let log = create_test_compilation_log(
            vec![], // No warnings in this compilation
            vec![CompilingModule {
                name: "Test".to_string(),
                path: "src/test.hs".into(),
            }],
        );

        tracker.update_warnings_from_log(&log);

        // Warnings should be kept since file was recompiled due to dependencies
        assert!(tracker.has_warnings());
        assert_eq!(tracker.warning_count(), 1);
    }

    #[test]
    fn test_edge_case_empty_file_path() {
        let mut tracker = WarningTracker::new();

        let diagnostic_no_path = GhcDiagnostic {
            severity: Severity::Warning,
            path: None,
            span: PositionRange::new(1, 1, 1, 1),
            message: "warning without path".to_string(),
        };

        let log = create_test_compilation_log(vec![diagnostic_no_path], vec![]);

        tracker.update_warnings_from_log(&log);

        // Should handle diagnostics without paths gracefully
        assert!(!tracker.has_warnings());
    }

    #[test]
    fn test_edge_case_large_number_of_warnings() {
        let mut tracker = WarningTracker::new();
        let base_dir = std::env::current_dir().unwrap();

        // Create 1000 warnings across 100 files
        for i in 0..100 {
            let path = NormalPath::new(format!("src/test{}.hs", i), &base_dir).unwrap();
            let mut warnings = Vec::new();
            for j in 0..10 {
                warnings.push(create_test_diagnostic(
                    Severity::Warning,
                    &format!("src/test{}.hs", i),
                    &format!("warning {} in file {}", j, i),
                ));
            }
            tracker.warnings.insert(path, warnings);
        }

        assert_eq!(tracker.warning_count(), 1000);
        assert_eq!(tracker.warnings.len(), 100);
    }

    #[test]
    fn test_edge_case_special_characters_in_paths() {
        let mut tracker = WarningTracker::new();
        let base_dir = std::env::current_dir().unwrap();

        // Test with various special characters that might appear in file paths
        let special_paths = vec![
            "src/test with spaces.hs",
            "src/test-with-dashes.hs",
            "src/test_with_underscores.hs",
            "src/test.with.dots.hs",
        ];

        for path_str in special_paths {
            if let Ok(path) = NormalPath::new(path_str, &base_dir) {
                let warning = create_test_diagnostic(Severity::Warning, path_str, "test warning");
                tracker.warnings.insert(path, vec![warning]);
            }
        }

        assert!(!tracker.warnings.is_empty());
    }
}
