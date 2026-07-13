use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct SourceLine {
    pub line_no: usize,
    pub indent: usize,
    pub text: String,
}

// ... rest will be in main.rs for now to avoid circular deps
