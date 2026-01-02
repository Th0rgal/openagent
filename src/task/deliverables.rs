//! Deliverable extraction from user prompts.
//!
//! Parses user messages to identify expected deliverables (files, reports, etc.)
//! that must exist for a task to be considered complete.

use regex::Regex;
use std::path::PathBuf;

/// A deliverable that the user expects from the task.
#[derive(Debug, Clone, PartialEq)]
pub enum Deliverable {
    /// A file that should be created at a specific path.
    File {
        path: PathBuf,
        description: Option<String>,
    },
    /// A directory that should be created.
    Directory { path: PathBuf },
    /// A report or document (may be in final message or a file).
    Report {
        topic: String,
        expected_path: Option<PathBuf>,
    },
}

impl Deliverable {
    /// Get the path if this deliverable has one.
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Deliverable::File { path, .. } => Some(path),
            Deliverable::Directory { path } => Some(path),
            Deliverable::Report { expected_path, .. } => expected_path.as_ref(),
        }
    }

    /// Check if this deliverable exists on the filesystem.
    pub async fn exists(&self) -> bool {
        match self {
            Deliverable::File { path, .. } => tokio::fs::metadata(path).await.is_ok(),
            Deliverable::Directory { path } => tokio::fs::metadata(path)
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false),
            Deliverable::Report { expected_path, .. } => {
                if let Some(path) = expected_path {
                    tokio::fs::metadata(path).await.is_ok()
                } else {
                    // Reports without explicit paths are delivered in the message
                    true
                }
            }
        }
    }
}

/// Result of deliverable extraction.
#[derive(Debug, Clone, Default)]
pub struct DeliverableSet {
    pub deliverables: Vec<Deliverable>,
    /// Keywords that suggest the task is research/analysis (may not have file deliverables)
    pub is_research_task: bool,
    /// Keywords that suggest the task requires a report
    pub requires_report: bool,
}

impl DeliverableSet {
    /// Check which deliverables are still missing.
    pub async fn missing(&self) -> Vec<&Deliverable> {
        let mut missing = Vec::new();
        for d in &self.deliverables {
            if !d.exists().await {
                missing.push(d);
            }
        }
        missing
    }

    /// Check if all deliverables exist.
    pub async fn all_complete(&self) -> bool {
        for d in &self.deliverables {
            if !d.exists().await {
                return false;
            }
        }
        true
    }

    /// Get paths of missing deliverables.
    pub async fn missing_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for d in &self.deliverables {
            if !d.exists().await {
                if let Some(path) = d.path() {
                    paths.push(path.display().to_string());
                }
            }
        }
        paths
    }
}

/// Extract expected deliverables from a user message.
///
/// Looks for patterns like:
/// - "create a report at /path/to/file.md"
/// - "save output to /root/work/task/output.json"
/// - "write the results to /path"
/// - "/root/work/project/output/REPORT.md" (explicit paths)
pub fn extract_deliverables(message: &str) -> DeliverableSet {
    let mut deliverables = Vec::new();
    let mut is_research_task = false;
    let mut requires_report = false;

    // Check for research/analysis keywords
    let research_keywords = [
        "research",
        "analyze",
        "investigate",
        "study",
        "explore",
        "find out",
    ];
    for keyword in &research_keywords {
        if message.to_lowercase().contains(keyword) {
            is_research_task = true;
            break;
        }
    }

    // Check for report requirement
    let report_keywords = ["report", "summary", "findings", "analysis", "documentation"];
    for keyword in &report_keywords {
        if message.to_lowercase().contains(keyword) {
            requires_report = true;
            break;
        }
    }

    // Pattern 1: Explicit paths with create/write/save verbs
    // Matches: "create report at /path/file.md", "write to /path/file", "save output to /path"
    let verb_path_pattern = Regex::new(
        r"(?i)(?:create|write|save|output|generate|produce|put|store)(?:\s+\w+)*?\s+(?:at|to|in)\s+(/[\w/.+-]+)"
    ).unwrap();

    for cap in verb_path_pattern.captures_iter(message) {
        let path = PathBuf::from(&cap[1]);
        if !deliverables
            .iter()
            .any(|d: &Deliverable| d.path() == Some(&path))
        {
            deliverables.push(Deliverable::File {
                path,
                description: None,
            });
        }
    }

    // Pattern 2: Explicit file paths in the message (especially in /root/work)
    // Matches: /root/work/project/output/REPORT.md
    let explicit_path_pattern =
        Regex::new(r"(/root/[\w/.+-]+\.(?:md|json|txt|py|sh|yaml|yml|csv|html|xml))").unwrap();

    for cap in explicit_path_pattern.captures_iter(message) {
        let path = PathBuf::from(&cap[1]);
        if !deliverables
            .iter()
            .any(|d: &Deliverable| d.path() == Some(&path))
        {
            deliverables.push(Deliverable::File {
                path,
                description: None,
            });
        }
    }

    // Pattern 3: "Deliverable:" or "Output:" sections
    let deliverable_section_pattern =
        Regex::new(r"(?i)(?:deliverable|output|result)s?:\s*\n(?:[-*]\s*)?(/[\w/.+-]+)").unwrap();

    for cap in deliverable_section_pattern.captures_iter(message) {
        let path = PathBuf::from(&cap[1]);
        if !deliverables
            .iter()
            .any(|d: &Deliverable| d.path() == Some(&path))
        {
            deliverables.push(Deliverable::File {
                path,
                description: None,
            });
        }
    }

    // Pattern 4: Directory patterns like "clone to /path/dir"
    let dir_pattern =
        Regex::new(r"(?i)(?:clone|download|extract)(?:\s+\w+)*?\s+(?:to|into)\s+(/[\w/.+-]+)")
            .unwrap();

    for cap in dir_pattern.captures_iter(message) {
        let path = PathBuf::from(&cap[1]);
        // If it doesn't have an extension, treat as directory
        if path.extension().is_none() {
            if !deliverables
                .iter()
                .any(|d: &Deliverable| d.path() == Some(&path))
            {
                deliverables.push(Deliverable::Directory { path });
            }
        }
    }

    // If requires_report but no explicit path found, add a generic report expectation
    if requires_report
        && !deliverables
            .iter()
            .any(|d| matches!(d, Deliverable::Report { .. }))
    {
        // Look for a topic
        let topic = if let Some(cap) = Regex::new(r"(?i)(?:about|on|regarding)\s+(.+?)(?:\.|,|$)")
            .unwrap()
            .captures(message)
        {
            cap[1].trim().to_string()
        } else {
            "the requested topic".to_string()
        };

        // Check if there's an output path for the report
        let expected_path = deliverables
            .iter()
            .find(|d: &&Deliverable| {
                if let Deliverable::File { path, .. } = d {
                    path.extension().map(|e| e == "md").unwrap_or(false)
                } else {
                    false
                }
            })
            .and_then(|d: &Deliverable| d.path().cloned());

        if expected_path.is_none() {
            deliverables.push(Deliverable::Report {
                topic,
                expected_path: None,
            });
        }
    }

    DeliverableSet {
        deliverables,
        is_research_task,
        requires_report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_explicit_path() {
        let msg = "Create a report at /root/work/oraxen-folia/output/REPORT.md";
        let result = extract_deliverables(msg);
        assert_eq!(result.deliverables.len(), 1);
        assert_eq!(
            result.deliverables[0].path().unwrap().to_str().unwrap(),
            "/root/work/oraxen-folia/output/REPORT.md"
        );
    }

    #[test]
    fn test_extract_inline_path() {
        let msg = "The final report should be saved to /root/work/analysis/findings.md";
        let result = extract_deliverables(msg);
        assert!(result.deliverables.iter().any(|d| {
            d.path()
                .map(|p| p.to_str().unwrap().contains("findings.md"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn test_research_task_detection() {
        let msg = "Research what needs to be done to support Folia";
        let result = extract_deliverables(msg);
        assert!(result.is_research_task);
    }

    #[test]
    fn test_report_requirement() {
        let msg = "Create a detailed report about the security vulnerabilities";
        let result = extract_deliverables(msg);
        assert!(result.requires_report);
    }

    #[test]
    fn test_multiple_deliverables() {
        let msg = r#"
Tasks:
1. Clone to /root/work/project/repo
2. Create report at /root/work/project/output/REPORT.md
3. Save analysis to /root/work/project/output/analysis.json
"#;
        let result = extract_deliverables(msg);
        assert!(result.deliverables.len() >= 2);
    }
}
