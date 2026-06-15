use lenso::{
    AdminSchema, EntitySchema, FieldSchema, FieldType, ModuleManifest, ModuleManifestLintSeverity,
    ModuleSource, RuntimeFunctionDeclaration, RuntimeSurface, lint_module_manifest,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = ModuleManifest::builder("rust-notes")
        .capabilities(vec!["rust-notes.entries.read".to_owned()])
        .admin(AdminSchema {
            entities: vec![EntitySchema {
                name: "entries".to_owned(),
                label: "Entries".to_owned(),
                read_capability: "rust-notes.entries.read".to_owned(),
                fields: vec![
                    FieldSchema {
                        name: "id".to_owned(),
                        label: "ID".to_owned(),
                        field_type: FieldType::String,
                        nullable: false,
                    },
                    FieldSchema {
                        name: "title".to_owned(),
                        label: "Title".to_owned(),
                        field_type: FieldType::String,
                        nullable: false,
                    },
                    FieldSchema {
                        name: "created_at".to_owned(),
                        label: "Created".to_owned(),
                        field_type: FieldType::Timestamp,
                        nullable: false,
                    },
                ],
            }],
        })
        .runtime(RuntimeSurface {
            functions: vec![RuntimeFunctionDeclaration {
                name: "rust-notes.sync.v1".to_owned(),
                version: 1,
                queue: "rust-notes".to_owned(),
                input_schema: Some("rust-notes.sync.v1".to_owned()),
                retry_policy: None,
            }],
        })
        .build();

    let lints = lint_module_manifest(ModuleSource::Remote, &manifest);
    let has_errors = lints
        .iter()
        .any(|lint| matches!(lint.severity, ModuleManifestLintSeverity::Error));

    if has_errors {
        for lint in lints {
            eprintln!(
                "{:?}: {} - {} ({})",
                lint.severity, lint.subject, lint.message, lint.suggestion
            );
        }
        std::process::exit(1);
    }

    println!("{}", serde_json::to_string_pretty(&manifest)?);

    Ok(())
}
