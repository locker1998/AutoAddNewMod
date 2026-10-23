#[derive(Debug, Clone)]
pub enum Stmt {
    Field {
        name: String,
        line: usize,
    },
    Var {
        name: String,
        value: Expr,
        line: usize,
    },
    Assign {
        name: String,
        value: Expr,
        line: usize,
    },
    If {
        cond: Expr,
        body: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
        line: usize,
    },
    Return {
        value: Expr,
        line: usize,
    },
    Expr {
        expr: Expr,
        line: usize,
    },
}

#[derive(Debug, Clone)]
pub enum Expr {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Ident(String),
    List(Vec<Expr>),
    Map(Vec<(String, Expr)>),
    UnaryNot(Box<Expr>),
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Prop {
        object: Box<Expr>,
        name: String,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        end: Option<Box<Expr>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
}
