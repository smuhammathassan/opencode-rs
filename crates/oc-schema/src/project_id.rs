//! From reference/packages/schema/src/project-id.ts

/// `Schema.String.pipe(Schema.brand("Project.ID"))`.
pub type ProjectID = String;

/// `ProjectID.global`.
pub fn global() -> ProjectID {
    "global".to_string()
}
