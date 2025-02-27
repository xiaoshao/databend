// Copyright 2022 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use common_ast::ast::BinaryOperator;
use common_ast::ast::IntervalKind;
use common_ast::ast::Literal as ASTLiteral;
use common_ast::ast::TypeName;
use common_ast::ast::UnaryOperator;
use common_ast::parser::parse_expr;
use common_ast::parser::token::Token;
use common_ast::parser::tokenize_sql;
use common_ast::Backtrace;
use common_ast::Dialect;
use common_expression::types::DataType;
use common_expression::types::NumberDataType;
use common_expression::Literal;
use common_expression::RawExpr;
use common_expression::Span;

pub fn parse_raw_expr(text: &str, columns: &[(&str, DataType)]) -> RawExpr {
    let backtrace = Backtrace::new();
    let tokens = tokenize_sql(text).unwrap();
    let expr = parse_expr(&tokens, Dialect::PostgreSQL, &backtrace).unwrap();
    transform_expr(expr, columns)
}

macro_rules! with_interval_mapped_name {
    (| $t:tt | $($tail:tt)*) => {
        match_template::match_template! {
            $t = [
              Year => "years", Quarter => "quarters", Month => "months", Day => "days",
              Hour => "hours", Minute => "minutes", Second => "seconds",
            ],
            $($tail)*
        }
    }
}

macro_rules! transform_interval_add_sub {
    ($span: expr, $columns: expr, $name: expr, $unit: expr, $date: expr, $interval: expr) => {
        if $name == "plus" {
            with_interval_mapped_name!(|INTERVAL| match $unit {
                IntervalKind::INTERVAL => RawExpr::FunctionCall {
                    span: transform_span($span),
                    name: concat!("add_", INTERVAL).to_string(),
                    params: vec![],
                    args: vec![
                        transform_expr(*$date, $columns),
                        transform_expr(*$interval, $columns),
                    ],
                },
                kind => {
                    unimplemented!("{kind:?} is not supported for interval")
                }
            })
        } else if $name == "minus" {
            with_interval_mapped_name!(|INTERVAL| match $unit {
                IntervalKind::INTERVAL => RawExpr::FunctionCall {
                    span: transform_span($span),
                    name: concat!("subtract_", INTERVAL).to_string(),
                    params: vec![],
                    args: vec![
                        transform_expr(*$date, $columns),
                        transform_expr(*$interval, $columns),
                    ],
                },
                kind => {
                    unimplemented!("{kind:?} is not supported for interval")
                }
            })
        } else {
            unimplemented!("operator {} is not supported for interval", $name)
        }
    };
}

pub fn transform_expr(ast: common_ast::ast::Expr, columns: &[(&str, DataType)]) -> RawExpr {
    match ast {
        common_ast::ast::Expr::Literal { span, lit } => RawExpr::Literal {
            span: transform_span(span),
            lit: transform_literal(lit),
        },
        common_ast::ast::Expr::ColumnRef {
            span,
            database: None,
            table: None,
            column,
        } => {
            let col_id = columns
                .iter()
                .position(|(col_name, _)| *col_name == column.name)
                .unwrap_or_else(|| panic!("expected column {}", column.name));
            RawExpr::ColumnRef {
                span: transform_span(span),
                id: col_id,
                data_type: columns[col_id].1.clone(),
            }
        }
        common_ast::ast::Expr::Cast {
            span,
            expr,
            target_type,
            ..
        } => match target_type {
            TypeName::Timestamp { .. } => RawExpr::FunctionCall {
                span: transform_span(span),
                name: "to_timestamp".to_string(),
                args: vec![transform_expr(*expr, columns)],
                params: vec![],
            },
            TypeName::Date => RawExpr::FunctionCall {
                span: transform_span(span),
                name: "to_date".to_string(),
                args: vec![transform_expr(*expr, columns)],
                params: vec![],
            },
            _ => RawExpr::Cast {
                span: transform_span(span),
                expr: Box::new(transform_expr(*expr, columns)),
                dest_type: transform_data_type(target_type),
            },
        },
        common_ast::ast::Expr::TryCast {
            span,
            expr,
            target_type,
            ..
        } => match target_type {
            TypeName::Timestamp { .. } => RawExpr::FunctionCall {
                span: transform_span(span),
                name: "try_to_timestamp".to_string(),
                args: vec![transform_expr(*expr, columns)],
                params: vec![],
            },
            TypeName::Date => RawExpr::FunctionCall {
                span: transform_span(span),
                name: "try_to_date".to_string(),
                args: vec![transform_expr(*expr, columns)],
                params: vec![],
            },
            _ => RawExpr::TryCast {
                span: transform_span(span),
                expr: Box::new(transform_expr(*expr, columns)),
                dest_type: transform_data_type(target_type),
            },
        },
        common_ast::ast::Expr::FunctionCall {
            span,
            name,
            args,
            params,
            ..
        } => RawExpr::FunctionCall {
            span: transform_span(span),
            name: name.name,
            args: args
                .into_iter()
                .map(|arg| transform_expr(arg, columns))
                .collect(),
            params: params
                .into_iter()
                .map(|param| match param {
                    ASTLiteral::Integer(u) => u as usize,
                    _ => unimplemented!(),
                })
                .collect(),
        },
        common_ast::ast::Expr::UnaryOp { span, op, expr } => RawExpr::FunctionCall {
            span: transform_span(span),
            name: transform_unary_op(op),
            params: vec![],
            args: vec![transform_expr(*expr, columns)],
        },
        common_ast::ast::Expr::BinaryOp {
            span,
            op,
            left,
            right,
        } => {
            let name = transform_binary_op(op);
            match name.as_str() {
                "notlike" => {
                    let result = RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "like".to_string(),
                        params: vec![],
                        args: vec![
                            transform_expr(*left, columns),
                            transform_expr(*right, columns),
                        ],
                    };
                    RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "not".to_string(),
                        params: vec![],
                        args: vec![result],
                    }
                }
                "notregexp" | "notrlike" => {
                    let result = RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "regexp".to_string(),
                        params: vec![],
                        args: vec![
                            transform_expr(*left, columns),
                            transform_expr(*right, columns),
                        ],
                    };
                    RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "not".to_string(),
                        params: vec![],
                        args: vec![result],
                    }
                }
                _ => match (*left.clone(), *right.clone()) {
                    (common_ast::ast::Expr::Interval { expr, unit, .. }, _) => {
                        if name == "minus" {
                            unimplemented!("interval cannot be the minuend")
                        } else {
                            transform_interval_add_sub!(span, columns, name, unit, right, expr)
                        }
                    }
                    (_, common_ast::ast::Expr::Interval { expr, unit, .. }) => {
                        transform_interval_add_sub!(span, columns, name, unit, left, expr)
                    }
                    (_, _) => RawExpr::FunctionCall {
                        span: transform_span(span),
                        name,
                        params: vec![],
                        args: vec![
                            transform_expr(*left, columns),
                            transform_expr(*right, columns),
                        ],
                    },
                },
            }
        }
        common_ast::ast::Expr::Position {
            span,
            substr_expr,
            str_expr,
        } => RawExpr::FunctionCall {
            span: transform_span(span),
            name: "position".to_string(),
            params: vec![],
            args: vec![
                transform_expr(*substr_expr, columns),
                transform_expr(*str_expr, columns),
            ],
        },
        common_ast::ast::Expr::Trim {
            span,
            expr,
            trim_where,
        } => {
            if let Some(inner) = trim_where {
                match inner.0 {
                    common_ast::ast::TrimWhere::Both => RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "trim_both".to_string(),
                        params: vec![],
                        args: vec![
                            transform_expr(*expr, columns),
                            transform_expr(*inner.1, columns),
                        ],
                    },
                    common_ast::ast::TrimWhere::Leading => RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "trim_leading".to_string(),
                        params: vec![],
                        args: vec![
                            transform_expr(*expr, columns),
                            transform_expr(*inner.1, columns),
                        ],
                    },
                    common_ast::ast::TrimWhere::Trailing => RawExpr::FunctionCall {
                        span: transform_span(span),
                        name: "trim_trailing".to_string(),
                        params: vec![],
                        args: vec![
                            transform_expr(*expr, columns),
                            transform_expr(*inner.1, columns),
                        ],
                    },
                }
            } else {
                RawExpr::FunctionCall {
                    span: transform_span(span),
                    name: "trim".to_string(),
                    params: vec![],
                    args: vec![transform_expr(*expr, columns)],
                }
            }
        }
        common_ast::ast::Expr::Substring {
            span,
            expr,
            substring_from,
            substring_for,
        } => {
            let mut args = vec![
                transform_expr(*expr, columns),
                transform_expr(*substring_from, columns),
            ];
            if let Some(substring_for) = substring_for {
                args.push(transform_expr(*substring_for, columns));
            }
            RawExpr::FunctionCall {
                span: transform_span(span),
                name: "substr".to_string(),
                params: vec![],
                args,
            }
        }
        common_ast::ast::Expr::Array { span, exprs } => RawExpr::FunctionCall {
            span: transform_span(span),
            name: "array".to_string(),
            params: vec![],
            args: exprs
                .into_iter()
                .map(|expr| transform_expr(expr, columns))
                .collect(),
        },
        common_ast::ast::Expr::IsNull { span, expr, not } => {
            let expr = transform_expr(*expr, columns);
            let result = RawExpr::FunctionCall {
                span: transform_span(span),
                name: "is_not_null".to_string(),
                params: vec![],
                args: vec![expr],
            };

            if not {
                result
            } else {
                RawExpr::FunctionCall {
                    span: transform_span(span),
                    name: "not".to_string(),
                    params: vec![],
                    args: vec![result],
                }
            }
        }
        common_ast::ast::Expr::DateAdd {
            span,
            unit,
            interval,
            date,
        } => {
            with_interval_mapped_name!(|INTERVAL| match unit {
                IntervalKind::INTERVAL => RawExpr::FunctionCall {
                    span: transform_span(span),
                    name: concat!("add_", INTERVAL).to_string(),
                    params: vec![],
                    args: vec![
                        transform_expr(*date, columns),
                        transform_expr(*interval, columns),
                    ],
                },
                kind => {
                    unimplemented!("{kind:?} is not supported")
                }
            })
        }
        common_ast::ast::Expr::DateSub {
            span,
            unit,
            interval,
            date,
        } => {
            with_interval_mapped_name!(|INTERVAL| match unit {
                IntervalKind::INTERVAL => RawExpr::FunctionCall {
                    span: transform_span(span),
                    name: concat!("subtract_", INTERVAL).to_string(),
                    params: vec![],
                    args: vec![
                        transform_expr(*date, columns),
                        transform_expr(*interval, columns),
                    ],
                },
                kind => {
                    unimplemented!("{kind:?} is not supported")
                }
            })
        }
        expr => unimplemented!("{expr:?} is unimplemented"),
    }
}

fn transform_unary_op(op: UnaryOperator) -> String {
    format!("{op:?}").to_lowercase()
}

fn transform_binary_op(op: BinaryOperator) -> String {
    format!("{op:?}").to_lowercase()
}

fn transform_data_type(target_type: common_ast::ast::TypeName) -> DataType {
    match target_type {
        common_ast::ast::TypeName::Boolean => DataType::Boolean,
        common_ast::ast::TypeName::UInt8 => DataType::Number(NumberDataType::UInt8),
        common_ast::ast::TypeName::UInt16 => DataType::Number(NumberDataType::UInt16),
        common_ast::ast::TypeName::UInt32 => DataType::Number(NumberDataType::UInt32),
        common_ast::ast::TypeName::UInt64 => DataType::Number(NumberDataType::UInt64),
        common_ast::ast::TypeName::Int8 => DataType::Number(NumberDataType::Int8),
        common_ast::ast::TypeName::Int16 => DataType::Number(NumberDataType::Int16),
        common_ast::ast::TypeName::Int32 => DataType::Number(NumberDataType::Int32),
        common_ast::ast::TypeName::Int64 => DataType::Number(NumberDataType::Int64),
        common_ast::ast::TypeName::Float32 => DataType::Number(NumberDataType::Float32),
        common_ast::ast::TypeName::Float64 => DataType::Number(NumberDataType::Float64),
        common_ast::ast::TypeName::String => DataType::String,
        common_ast::ast::TypeName::Timestamp => DataType::Timestamp,
        common_ast::ast::TypeName::Date => DataType::Date,
        common_ast::ast::TypeName::Array {
            item_type: Some(item_type),
        } => DataType::Array(Box::new(transform_data_type(*item_type))),
        common_ast::ast::TypeName::Tuple { fields_type, .. } => {
            DataType::Tuple(fields_type.into_iter().map(transform_data_type).collect())
        }
        common_ast::ast::TypeName::Nullable(inner_type) => {
            DataType::Nullable(Box::new(transform_data_type(*inner_type)))
        }
        common_ast::ast::TypeName::Variant => DataType::Variant,
        _ => unimplemented!(),
    }
}

pub fn transform_literal(lit: ASTLiteral) -> Literal {
    match lit {
        ASTLiteral::Integer(u) => {
            if u < u8::MAX as u64 {
                Literal::UInt8(u as u8)
            } else if u < u16::MAX as u64 {
                Literal::UInt16(u as u16)
            } else if u < u32::MAX as u64 {
                Literal::UInt32(u as u32)
            } else {
                Literal::UInt64(u)
            }
        }
        ASTLiteral::String(s) => Literal::String(s.as_bytes().to_vec()),
        ASTLiteral::Boolean(b) => Literal::Boolean(b),
        ASTLiteral::Null => Literal::Null,
        ASTLiteral::Float(f) => Literal::Float64(f),
        _ => unimplemented!("{lit}"),
    }
}

pub fn transform_span(span: &[Token]) -> Span {
    let start = span.first().unwrap().span.start;
    let end = span.last().unwrap().span.end;
    Some(start..end)
}
