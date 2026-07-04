#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourcePath {
    parts: Vec<PathPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathPart {
    Field(String),
    ArrayIndex(usize),
}

impl SourcePath {
    pub fn root() -> Self {
        Self { parts: Vec::new() }
    }

    pub fn push_field(&mut self, field: &str) {
        self.parts.push(PathPart::Field(field.to_string()));
    }

    pub fn push_index(&mut self, index: usize) {
        self.parts.push(PathPart::ArrayIndex(index));
    }

    pub fn pop(&mut self) {
        self.parts.pop();
    }

    pub fn as_str(&self) -> String {
        format_path(&self.parts, false)
    }

    pub fn to_canonical_guess(&self) -> String {
        format_path(&self.parts, true)
    }
}

fn format_path(parts: &[PathPart], canonicalize_indexes: bool) -> String {
    let mut path = String::from("$");
    for part in parts {
        match part {
            PathPart::Field(field) => {
                path.push('.');
                path.push_str(field);
            }
            PathPart::ArrayIndex(index) if canonicalize_indexes => {
                let _ = index;
                path.push_str("[]");
            }
            PathPart::ArrayIndex(index) => {
                path.push('[');
                path.push_str(&index.to_string());
                path.push(']');
            }
        }
    }
    path
}
