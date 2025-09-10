#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub imports: Vec<UseStatement>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseStatement {
    pub path: Vec<String>, // e.g., ["crate", "module", "submodule"]
    pub items: UseTree,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UseTree {
    Simple(String),        // use module::Item;
    Glob,                  // use module::*;
    List(Vec<UseTree>),    // use module::{Item1, Item2};
    Alias(String, String), // use module::Item as Alias;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Struct(Struct),
    Enum(Enum),
    Trait(Trait),
    Impl(Impl),
    Function(Function),
    Const(Const),
    Global(Statement),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub name: String,
    pub generics: Vec<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    pub name: String,
    pub generics: Vec<String>,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    pub fields: VariantFields,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantFields {
    Unit,
    Tuple(Vec<Type>),
    Named(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trait {
    pub name: String,
    pub generics: Vec<String>,
    pub methods: Vec<TraitMethod>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Impl {
    pub generics: Vec<String>,
    pub trait_name: Option<String>,
    pub target_type: Type,
    pub methods: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub is_self: bool,
    pub is_mut: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Const {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Path(String),
    Generic(String, Vec<Type>),
    Reference(Box<Type>, bool),
    Array(Box<Type>, usize),
    Tuple(Vec<Type>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let(LetStatement),
    Assign(Expr, Expr), // lhs, rhs
    Expr(Expr),
    Return(Option<Expr>),
    If(IfStatement),
    While(WhileStatement),
    For(ForStatement),
    Match(MatchStatement),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStatement {
    pub name: String,
    pub ty: Option<Type>,
    pub value: Option<Expr>,
    pub is_mut: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfStatement {
    pub condition: Expr,
    pub then_branch: Block,
    pub else_branch: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhileStatement {
    pub condition: Expr,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForStatement {
    pub var: String,
    pub iter: Expr,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchStatement {
    pub expr: Expr,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Ident(String),
    Literal(Literal),
    Struct(String, Vec<(String, Pattern)>),
    Enum(String, String, Option<Box<Pattern>>),
    Tuple(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Struct(String, Vec<(String, Expr)>),
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Block(Block),
    If(Box<IfStatement>),
    Match(Box<MatchStatement>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>),
    None,
    Some(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    Char(char),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}
